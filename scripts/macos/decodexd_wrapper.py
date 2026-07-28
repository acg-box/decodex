#!/usr/bin/env python3
"""Compose and inspect the fixed macOS decodexd application wrapper."""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import plistlib
import re
import selectors
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path
from typing import Any


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
PACKAGING_ROOT = REPOSITORY_ROOT / "apps/decodexd/packaging"
INFO_PLIST_SOURCE = PACKAGING_ROOT / "Info.plist"
ENTITLEMENTS_SOURCE = PACKAGING_ROOT / "decodexd.entitlements"

DESCRIPTOR_SCHEMA = "decodex/daemon-wrapper/1"
RESULT_SCHEMA = "decodex/daemon-wrapper-result/1"
WRAPPER_NAME = "decodexd.app"

BUNDLE_IDENTIFIER = "box.acg.decodex.daemon"
BUNDLE_EXECUTABLE = "decodexd"
BUNDLE_PACKAGE_TYPE = "APPL"
TEAM_IDENTIFIER = "T54QFA7W2S"
APPLICATION_IDENTIFIER = f"{TEAM_IDENTIFIER}.{BUNDLE_IDENTIFIER}"
PROFILE_CHANNEL = "development"
ACCESS_GROUPS = [APPLICATION_IDENTIFIER]

CODESIGN = Path("/usr/bin/codesign")
SECURITY = Path("/usr/bin/security")
TOOL_TIMEOUT_SECONDS = 30
MAX_TOOL_OUTPUT_BYTES = 2 * 1024 * 1024
MAX_EXECUTABLE_BYTES = 256 * 1024 * 1024
MAX_PROFILE_BYTES = 4 * 1024 * 1024
MAX_PLIST_BYTES = 128 * 1024
MAX_CERTIFICATE_BYTES = 128 * 1024
MAX_CERTIFICATE_SET_BYTES = 512 * 1024
MAX_CERTIFICATE_COUNT = 16
MAX_IDENTITY_BYTES = 512
HEX_PATTERN = re.compile(r"^[0-9a-fA-F]+$")
MACHO_MAGICS = {
    b"\xfe\xed\xfa\xce",
    b"\xce\xfa\xed\xfe",
    b"\xfe\xed\xfa\xcf",
    b"\xcf\xfa\xed\xfe",
    b"\xca\xfe\xba\xbe",
    b"\xbe\xba\xfe\xca",
    b"\xca\xfe\xba\xbf",
    b"\xbf\xba\xfe\xca",
}

EXPECTED_ENTITLEMENTS = {
    "com.apple.application-identifier": APPLICATION_IDENTIFIER,
    "com.apple.developer.team-identifier": TEAM_IDENTIFIER,
    "keychain-access-groups": ACCESS_GROUPS,
}

DESCRIPTOR_FIELDS = {
    "schema",
    "wrapper_path",
    "executable_path",
    "executable_sha256",
    "executable_byte_count",
    "info_plist_path",
    "info_plist_sha256",
    "bundle_identifier",
    "bundle_executable",
    "bundle_package_type",
    "background_only",
    "embedded_profile_path",
    "embedded_profile_sha256",
    "team_identifier",
    "application_identifier",
    "profile_expires_at",
    "profile_channel",
    "signed_entitlements_sha256",
    "keychain_access_groups",
    "signature_identity_sha256",
}


class WrapperError(RuntimeError):
    """A bounded, nonsecret wrapper refusal."""


class _ArgumentParser(argparse.ArgumentParser):
    def error(self, _message: str) -> None:
        raise WrapperError("daemon wrapper command arguments are invalid")


def canonical_json(value: Any) -> bytes:
    try:
        return json.dumps(
            value,
            ensure_ascii=True,
            allow_nan=False,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("ascii")
    except (TypeError, ValueError) as error:
        raise WrapperError("daemon wrapper descriptor is not canonical") from error


def canonical_sha256(value: Any) -> str:
    return hashlib.sha256(canonical_json(value)).hexdigest()


def sha256_bytes(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


def descriptor_path(value: Any, expected_name: str) -> Path:
    if not isinstance(value, str):
        raise WrapperError("daemon wrapper descriptor fields differ")
    path = Path(value)
    try:
        canonical = path.resolve(strict=True)
    except (OSError, RuntimeError) as error:
        raise WrapperError("daemon wrapper descriptor fields differ") from error
    if not path.is_absolute() or path.name != expected_name or canonical != path:
        raise WrapperError("daemon wrapper descriptor fields differ")
    return path


def validate_descriptor(descriptor: dict[str, Any]) -> None:
    digest_fields = (
        "executable_sha256",
        "info_plist_sha256",
        "embedded_profile_sha256",
        "signed_entitlements_sha256",
        "signature_identity_sha256",
    )
    wrapper_path = descriptor_path(descriptor.get("wrapper_path"), WRAPPER_NAME)
    executable_path = descriptor_path(
        descriptor.get("executable_path"),
        BUNDLE_EXECUTABLE,
    )
    if (
        set(descriptor) != DESCRIPTOR_FIELDS
        or descriptor.get("schema") != DESCRIPTOR_SCHEMA
        or executable_path
        != wrapper_path / "Contents/MacOS" / BUNDLE_EXECUTABLE
        or type(descriptor.get("executable_byte_count")) is not int
        or descriptor["executable_byte_count"] <= 0
        or descriptor.get("info_plist_path")
        != str(wrapper_path / "Contents/Info.plist")
        or descriptor.get("bundle_identifier") != BUNDLE_IDENTIFIER
        or descriptor.get("bundle_executable") != BUNDLE_EXECUTABLE
        or descriptor.get("bundle_package_type") != BUNDLE_PACKAGE_TYPE
        or descriptor.get("background_only") is not True
        or descriptor.get("embedded_profile_path")
        != str(wrapper_path / "Contents/embedded.provisionprofile")
        or descriptor.get("team_identifier") != TEAM_IDENTIFIER
        or descriptor.get("application_identifier") != APPLICATION_IDENTIFIER
        or descriptor.get("profile_channel") != PROFILE_CHANNEL
        or descriptor.get("keychain_access_groups") != ACCESS_GROUPS
        or not isinstance(descriptor.get("profile_expires_at"), str)
        or re.fullmatch(
            r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z",
            descriptor["profile_expires_at"],
        )
        is None
        or any(
            not isinstance(descriptor.get(field), str)
            or re.fullmatch(r"[0-9a-f]{64}", descriptor[field]) is None
            for field in digest_fields
        )
    ):
        raise WrapperError("daemon wrapper descriptor fields differ")


def load_descriptor(body: bytes) -> dict[str, Any]:
    if len(body) > MAX_TOOL_OUTPUT_BYTES:
        raise WrapperError("daemon wrapper descriptor exceeded its bound")

    def object_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        value: dict[str, Any] = {}
        for key, member in pairs:
            if key in value:
                raise WrapperError("daemon wrapper descriptor has duplicate fields")
            value[key] = member
        return value

    try:
        value = json.loads(body, object_pairs_hook=object_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise WrapperError("daemon wrapper descriptor is malformed") from error
    if not isinstance(value, dict):
        raise WrapperError("daemon wrapper descriptor is malformed")
    validate_descriptor(value)
    return value


def _reject_duplicate_plist_keys(body: bytes) -> None:
    try:
        root = ET.fromstring(body)
    except ET.ParseError as error:
        raise WrapperError("daemon wrapper property list is malformed") from error
    if root.tag != "plist" or len(root) != 1:
        raise WrapperError("daemon wrapper property list is malformed")

    def inspect(element: ET.Element) -> None:
        if element.tag == "dict":
            children = list(element)
            if len(children) % 2 != 0:
                raise WrapperError("daemon wrapper property list is malformed")
            keys: set[str] = set()
            for index in range(0, len(children), 2):
                key = children[index]
                if key.tag != "key" or key.text is None or key.text in keys:
                    raise WrapperError(
                        "daemon wrapper property list has duplicate or invalid fields"
                    )
                keys.add(key.text)
                inspect(children[index + 1])
        elif element.tag == "array":
            for child in element:
                inspect(child)

    inspect(root[0])


def load_plist(body: bytes) -> dict[str, Any]:
    if len(body) > MAX_TOOL_OUTPUT_BYTES:
        raise WrapperError("daemon wrapper property list exceeded its bound")
    _reject_duplicate_plist_keys(body)
    try:
        value = plistlib.loads(body)
    except (ValueError, plistlib.InvalidFileException) as error:
        raise WrapperError("daemon wrapper property list is malformed") from error
    if not isinstance(value, dict) or not all(
        isinstance(key, str) for key in value
    ):
        raise WrapperError("daemon wrapper property list is malformed")
    return value


def read_regular(
    path: Path,
    maximum: int,
    failure: str,
    *,
    executable: bool | None = None,
) -> bytes:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise WrapperError(failure) from error
    if (
        not stat.S_ISREG(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_nlink != 1
        or metadata.st_size > maximum
        or (
            executable is not None
            and bool(metadata.st_mode & 0o111) is not executable
        )
    ):
        raise WrapperError(failure)
    flags = os.O_RDONLY | getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise WrapperError(failure) from error
    try:
        opened = os.fstat(descriptor)
        if (
            not stat.S_ISREG(opened.st_mode)
            or opened.st_nlink != 1
            or (opened.st_dev, opened.st_ino)
            != (metadata.st_dev, metadata.st_ino)
            or (
                executable is not None
                and bool(opened.st_mode & 0o111) is not executable
            )
        ):
            raise WrapperError(failure)
        chunks: list[bytes] = []
        size = 0
        while True:
            chunk = os.read(descriptor, min(64 * 1024, maximum + 1 - size))
            if not chunk:
                break
            size += len(chunk)
            if size > maximum:
                raise WrapperError(failure)
            chunks.append(chunk)
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def write_exclusive(path: Path, body: bytes, mode: int) -> None:
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_CLOEXEC", 0)
    flags |= getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags, mode)
    except OSError as error:
        raise WrapperError("daemon wrapper output could not be created") from error
    try:
        remaining = memoryview(body)
        while remaining:
            written = os.write(descriptor, remaining)
            if written <= 0:
                raise WrapperError("daemon wrapper output could not be created")
            remaining = remaining[written:]
        os.fchmod(descriptor, mode)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def require_safe_output(output: Path) -> None:
    if output.name != WRAPPER_NAME or not output.is_absolute():
        raise WrapperError("daemon wrapper output path is invalid")
    try:
        if output.exists() or output.is_symlink():
            raise WrapperError("daemon wrapper output already exists")
        parent = output.parent
        if parent.resolve(strict=True) != parent:
            raise WrapperError("daemon wrapper output parent is unsafe")
        metadata = parent.lstat()
    except WrapperError:
        raise
    except OSError as error:
        raise WrapperError("daemon wrapper output parent is unsafe") from error
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or stat.S_ISLNK(metadata.st_mode)
        or metadata.st_uid != os.geteuid()
        or metadata.st_mode & 0o022
    ):
        raise WrapperError("daemon wrapper output parent is unsafe")


def run_tool(command: list[str]) -> tuple[bytes, bytes]:
    process: subprocess.Popen[bytes] | None = None
    selector: selectors.BaseSelector | None = None
    streams: dict[int, tuple[str, Any]] = {}
    chunks: dict[str, list[bytes]] = {"stdout": [], "stderr": []}
    try:
        process = subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            close_fds=True,
        )
        if process.stdout is None or process.stderr is None:
            raise WrapperError("daemon wrapper platform tool pipes are unavailable")
        streams = {
            process.stdout.fileno(): ("stdout", process.stdout),
            process.stderr.fileno(): ("stderr", process.stderr),
        }
        selector = selectors.DefaultSelector()
        for descriptor in streams:
            os.set_blocking(descriptor, False)
            selector.register(descriptor, selectors.EVENT_READ)
        total = 0
        deadline = time.monotonic() + TOOL_TIMEOUT_SECONDS
        while selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise WrapperError("daemon wrapper platform tool timed out")
            for key, _ in selector.select(min(0.25, remaining)):
                name, stream = streams[key.fd]
                try:
                    chunk = os.read(key.fd, 64 * 1024)
                except BlockingIOError:
                    continue
                if not chunk:
                    selector.unregister(key.fd)
                    stream.close()
                    continue
                total += len(chunk)
                if total > MAX_TOOL_OUTPUT_BYTES:
                    raise WrapperError("daemon wrapper platform tool output exceeded its bound")
                chunks[name].append(chunk)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise WrapperError("daemon wrapper platform tool timed out")
        process.wait(timeout=remaining)
    except BaseException as primary_error:
        cleanup_error: BaseException | None = None
        if process is not None and process.poll() is None:
            try:
                process.kill()
                process.wait(timeout=10)
            except BaseException as error:
                cleanup_error = error
        if selector is not None:
            try:
                selector.close()
            except BaseException as error:
                cleanup_error = cleanup_error or error
        for _, stream in streams.values():
            if stream.closed:
                continue
            try:
                stream.close()
            except BaseException as error:
                cleanup_error = cleanup_error or error
        if isinstance(primary_error, (OSError, subprocess.TimeoutExpired)):
            failure = WrapperError("daemon wrapper platform tool failed")
            failure.__cause__ = primary_error
            primary_error = failure
        if cleanup_error is not None:
            raise primary_error from cleanup_error
        raise primary_error
    assert process is not None
    if selector is not None:
        selector.close()
    for _, stream in streams.values():
        if not stream.closed:
            stream.close()
    if process.returncode != 0:
        raise WrapperError("daemon wrapper platform tool failed")
    return b"".join(chunks["stdout"]), b"".join(chunks["stderr"])


def _profile_document(profile: Path) -> tuple[bytes, dict[str, Any]]:
    stdout, _ = run_tool([str(SECURITY), "cms", "-D", "-i", str(profile)])
    return stdout, load_plist(stdout)


def _normalized_expiry(value: Any) -> str:
    if not isinstance(value, datetime.datetime):
        raise WrapperError("daemon wrapper profile expiry is invalid")
    if value.tzinfo is None:
        value = value.replace(tzinfo=datetime.timezone.utc)
    value = value.astimezone(datetime.timezone.utc).replace(microsecond=0)
    if value <= datetime.datetime.now(datetime.timezone.utc):
        raise WrapperError("daemon wrapper profile is expired")
    return value.strftime("%Y-%m-%dT%H:%M:%SZ")


def _is_bounded_der_certificate(body: bytes) -> bool:
    if len(body) < 4 or len(body) > MAX_CERTIFICATE_BYTES or body[0] != 0x30:
        return False
    first_length = body[1]
    if first_length < 0x80:
        header_length = 2
        content_length = first_length
    else:
        length_bytes = first_length & 0x7F
        if (
            length_bytes == 0
            or length_bytes > 4
            or len(body) < 2 + length_bytes
            or body[2] == 0
        ):
            return False
        content_length = int.from_bytes(body[2 : 2 + length_bytes], "big")
        if content_length < 0x80:
            return False
        header_length = 2 + length_bytes
    return (
        header_length + content_length == len(body)
        and content_length > 0
        and body[header_length] == 0x30
    )


def _developer_certificates(profile: dict[str, Any]) -> frozenset[bytes]:
    certificates = profile.get("DeveloperCertificates")
    if (
        not isinstance(certificates, list)
        or not certificates
        or len(certificates) > MAX_CERTIFICATE_COUNT
        or not all(
            isinstance(certificate, bytes)
            and _is_bounded_der_certificate(certificate)
            for certificate in certificates
        )
        or sum(len(certificate) for certificate in certificates)
        > MAX_CERTIFICATE_SET_BYTES
    ):
        raise WrapperError("daemon wrapper profile certificates are invalid")
    certificate_set = frozenset(certificates)
    if len(certificate_set) != len(certificates):
        raise WrapperError("daemon wrapper profile certificates are invalid")
    return certificate_set


def validate_profile(
    profile: dict[str, Any],
) -> tuple[dict[str, Any], frozenset[bytes]]:
    team = profile.get("TeamIdentifier")
    prefixes = profile.get("ApplicationIdentifierPrefix")
    entitlements = profile.get("Entitlements")
    devices = profile.get("ProvisionedDevices")
    if (
        team != [TEAM_IDENTIFIER]
        or prefixes != [TEAM_IDENTIFIER]
        or not isinstance(entitlements, dict)
        or entitlements.get("com.apple.application-identifier")
        != APPLICATION_IDENTIFIER
        or entitlements.get("com.apple.developer.team-identifier")
        != TEAM_IDENTIFIER
        or entitlements.get("keychain-access-groups") != ACCESS_GROUPS
        or entitlements.get("get-task-allow") is not True
        or not isinstance(devices, list)
        or not devices
        or not all(isinstance(device, str) and device for device in devices)
        or profile.get("ProvisionsAllDevices") is True
    ):
        raise WrapperError("daemon wrapper profile identity is invalid")
    return (
        {
            "team_identifier": TEAM_IDENTIFIER,
            "application_identifier": APPLICATION_IDENTIFIER,
            "profile_expires_at": _normalized_expiry(
                profile.get("ExpirationDate")
            ),
            "profile_channel": PROFILE_CHANNEL,
        },
        _developer_certificates(profile),
    )


def _extract_plist_output(stdout: bytes, stderr: bytes) -> bytes:
    for body in (stdout, stderr):
        start = body.find(b"<?xml")
        if start >= 0:
            return body[start:]
    raise WrapperError("daemon wrapper signed entitlements are unavailable")


def signed_entitlements(wrapper: Path) -> dict[str, Any]:
    stdout, stderr = run_tool(
        [
            str(CODESIGN),
            "-d",
            "--entitlements",
            ":-",
            "--xml",
            str(wrapper),
        ]
    )
    entitlements = load_plist(_extract_plist_output(stdout, stderr))
    if entitlements != EXPECTED_ENTITLEMENTS:
        raise WrapperError("daemon wrapper signed entitlements differ")
    return entitlements


def _extracted_leaf_certificate(wrapper: Path) -> bytes:
    try:
        temporary = tempfile.TemporaryDirectory(prefix="decodexd-certificates-")
    except OSError as error:
        raise WrapperError(
            "daemon wrapper leaf certificate is unavailable"
        ) from error
    prefix = Path(temporary.name) / "certificate"

    def extract() -> bytes:
        run_tool(
            [
                str(CODESIGN),
                "-d",
                "--extract-certificates",
                str(prefix),
                str(wrapper),
            ]
        )
        try:
            members = list(Path(temporary.name).iterdir())
        except OSError as error:
            raise WrapperError(
                "daemon wrapper leaf certificate is unavailable"
            ) from error
        if not members or len(members) > MAX_CERTIFICATE_COUNT:
            raise WrapperError("daemon wrapper leaf certificate is unavailable")
        indexed: dict[int, Path] = {}
        for member in members:
            suffix = member.name.removeprefix(prefix.name)
            if (
                not member.name.startswith(prefix.name)
                or not suffix.isascii()
                or not suffix.isdecimal()
                or (suffix != "0" and suffix.startswith("0"))
            ):
                raise WrapperError(
                    "daemon wrapper leaf certificate is unavailable"
                )
            index = int(suffix)
            if index in indexed:
                raise WrapperError(
                    "daemon wrapper leaf certificate is unavailable"
                )
            indexed[index] = member
        if set(indexed) != set(range(len(indexed))):
            raise WrapperError("daemon wrapper leaf certificate is unavailable")
        bodies = [
            read_regular(
                indexed[index],
                MAX_CERTIFICATE_BYTES,
                "daemon wrapper leaf certificate is unavailable",
                executable=False,
            )
            for index in range(len(indexed))
        ]
        if (
            sum(len(body) for body in bodies) > MAX_CERTIFICATE_SET_BYTES
            or not all(_is_bounded_der_certificate(body) for body in bodies)
        ):
            raise WrapperError("daemon wrapper leaf certificate is unavailable")
        return bodies[0]

    try:
        leaf = extract()
    except BaseException as primary_error:
        try:
            temporary.cleanup()
        except BaseException as cleanup_error:
            raise primary_error from cleanup_error
        raise
    try:
        temporary.cleanup()
    except OSError as error:
        raise WrapperError(
            "daemon wrapper leaf certificate cleanup failed"
        ) from error
    return leaf


def signature_identity(
    wrapper: Path,
    developer_certificates: frozenset[bytes],
) -> dict[str, Any]:
    leaf_certificate = _extracted_leaf_certificate(wrapper)
    if leaf_certificate not in developer_certificates:
        raise WrapperError("daemon wrapper signature certificate differs")
    _, details_bytes = run_tool(
        [str(CODESIGN), "-d", "--verbose=4", str(wrapper)]
    )
    _, requirement_bytes = run_tool(
        [str(CODESIGN), "-d", "-r-", str(wrapper)]
    )
    try:
        details = details_bytes.decode("utf-8", errors="strict").splitlines()
        requirements = requirement_bytes.decode("utf-8", errors="strict").splitlines()
    except UnicodeDecodeError as error:
        raise WrapperError("daemon wrapper signature identity is malformed") from error

    def singleton(prefix: str) -> str:
        values = [
            line[len(prefix) :].strip()
            for line in details
            if line.startswith(prefix)
        ]
        if len(values) != 1 or not values[0]:
            raise WrapperError("daemon wrapper signature identity is malformed")
        return values[0]

    identifier = singleton("Identifier=")
    team = singleton("TeamIdentifier=")
    cdhash = singleton("CDHash=")
    code_directory = singleton("CodeDirectory ")
    authorities = [
        line[len("Authority=") :].strip()
        for line in details
        if line.startswith("Authority=")
    ]
    requirements = [
        " ".join(line[len("designated =>") :].split())
        for line in requirements
        if line.startswith("designated =>")
    ]
    if (
        identifier != BUNDLE_IDENTIFIER
        or team != TEAM_IDENTIFIER
        or len(cdhash) not in {40, 64}
        or not HEX_PATTERN.fullmatch(cdhash)
        or "runtime" not in code_directory
        or not authorities
        or len(authorities) != len(set(authorities))
        or any(not authority for authority in authorities)
        or len(requirements) != 1
        or f'identifier "{BUNDLE_IDENTIFIER}"' not in requirements[0]
    ):
        raise WrapperError("daemon wrapper signature identity differs")
    return {
        "identifier": identifier,
        "team_identifier": team,
        "cdhash": cdhash.lower(),
        "code_directory": " ".join(code_directory.split()),
        "designated_requirement": requirements[0],
        "certificate_authorities": authorities,
        "leaf_certificate_sha256": sha256_bytes(leaf_certificate),
    }


def _require_directory(path: Path) -> None:
    try:
        metadata = path.lstat()
    except OSError as error:
        raise WrapperError("daemon wrapper layout is invalid") from error
    if not stat.S_ISDIR(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode):
        raise WrapperError("daemon wrapper layout is invalid")


def validate_layout(wrapper: Path, require_fixed_name: bool) -> None:
    if (
        not wrapper.is_absolute()
        or wrapper.resolve(strict=True) != wrapper
        or (require_fixed_name and wrapper.name != WRAPPER_NAME)
    ):
        raise WrapperError("daemon wrapper path is invalid")
    contents = wrapper / "Contents"
    macos = contents / "MacOS"
    signature = contents / "_CodeSignature"
    for directory in (wrapper, contents, macos, signature):
        _require_directory(directory)
    allowed_directories = {
        wrapper,
        contents,
        macos,
        signature,
    }
    allowed_files = {
        contents / "Info.plist",
        contents / "embedded.provisionprofile",
        macos / "decodexd",
        signature / "CodeResources",
    }
    observed_directories: set[Path] = set()
    observed_files: set[Path] = set()
    executable_files: set[Path] = set()
    try:
        for root, directories, files in os.walk(wrapper, followlinks=False):
            root_path = Path(root)
            observed_directories.add(root_path)
            for name in directories:
                child = root_path / name
                metadata = child.lstat()
                if stat.S_ISLNK(metadata.st_mode) or not stat.S_ISDIR(
                    metadata.st_mode
                ):
                    raise WrapperError("daemon wrapper layout is invalid")
            for name in files:
                child = root_path / name
                metadata = child.lstat()
                if (
                    stat.S_ISLNK(metadata.st_mode)
                    or not stat.S_ISREG(metadata.st_mode)
                    or metadata.st_nlink != 1
                ):
                    raise WrapperError("daemon wrapper layout is invalid")
                observed_files.add(child)
                if metadata.st_mode & 0o111:
                    executable_files.add(child)
    except OSError as error:
        raise WrapperError("daemon wrapper layout is invalid") from error
    if (
        observed_directories != allowed_directories
        or observed_files != allowed_files
        or executable_files != {macos / "decodexd"}
    ):
        raise WrapperError("daemon wrapper layout is invalid")


def _inspect_wrapper(wrapper: Path, require_fixed_name: bool) -> dict[str, Any]:
    validate_layout(wrapper, require_fixed_name)
    contents = wrapper / "Contents"
    executable = contents / "MacOS/decodexd"
    info_path = contents / "Info.plist"
    profile_path = contents / "embedded.provisionprofile"
    read_regular(
        contents / "_CodeSignature/CodeResources",
        MAX_PLIST_BYTES,
        "daemon wrapper CodeResources is invalid",
        executable=False,
    )
    executable_body = read_regular(
        executable,
        MAX_EXECUTABLE_BYTES,
        "daemon wrapper executable is invalid",
        executable=True,
    )
    if executable_body[:4] not in MACHO_MAGICS:
        raise WrapperError("daemon wrapper executable is invalid")
    info_body = read_regular(
        info_path,
        MAX_PLIST_BYTES,
        "daemon wrapper Info.plist is invalid",
        executable=False,
    )
    authority_info = read_regular(
        INFO_PLIST_SOURCE,
        MAX_PLIST_BYTES,
        "daemon wrapper Info.plist authority is invalid",
        executable=False,
    )
    info = load_plist(info_body)
    if info_body != authority_info or info != {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleExecutable": BUNDLE_EXECUTABLE,
        "CFBundleIdentifier": BUNDLE_IDENTIFIER,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": "decodexd",
        "CFBundlePackageType": BUNDLE_PACKAGE_TYPE,
        "LSBackgroundOnly": True,
    }:
        raise WrapperError("daemon wrapper Info.plist differs")
    profile_body = read_regular(
        profile_path,
        MAX_PROFILE_BYTES,
        "daemon wrapper profile is invalid",
        executable=False,
    )
    _, profile = _profile_document(profile_path)
    profile_identity, developer_certificates = validate_profile(profile)
    run_tool(
        [
            str(CODESIGN),
            "--verify",
            "--strict",
            "--all-architectures",
            "--verbose=2",
            str(wrapper),
        ]
    )
    entitlements = signed_entitlements(wrapper)
    identity = signature_identity(wrapper, developer_certificates)
    wrapper_path = str(wrapper)
    executable_path = str(executable)
    descriptor = {
        "schema": DESCRIPTOR_SCHEMA,
        "wrapper_path": wrapper_path,
        "executable_path": executable_path,
        "executable_sha256": sha256_bytes(executable_body),
        "executable_byte_count": len(executable_body),
        "info_plist_path": str(info_path),
        "info_plist_sha256": sha256_bytes(info_body),
        "bundle_identifier": BUNDLE_IDENTIFIER,
        "bundle_executable": BUNDLE_EXECUTABLE,
        "bundle_package_type": BUNDLE_PACKAGE_TYPE,
        "background_only": True,
        "embedded_profile_path": str(profile_path),
        "embedded_profile_sha256": sha256_bytes(profile_body),
        **profile_identity,
        "signed_entitlements_sha256": canonical_sha256(entitlements),
        "keychain_access_groups": ACCESS_GROUPS,
        "signature_identity_sha256": canonical_sha256(identity),
    }
    validate_descriptor(descriptor)
    return descriptor


def inspect_wrapper(wrapper: Path) -> dict[str, Any]:
    return _inspect_wrapper(wrapper, True)


def verify_wrapper(
    wrapper: Path,
    expected_descriptor: dict[str, Any],
) -> dict[str, Any]:
    validate_descriptor(expected_descriptor)
    current = inspect_wrapper(wrapper)
    if canonical_json(current) != canonical_json(expected_descriptor):
        raise WrapperError("daemon wrapper identity differs")
    return current


def _validate_signing_identity(identity: str) -> None:
    if (
        not identity
        or len(identity.encode("utf-8")) > MAX_IDENTITY_BYTES
        or identity == "-"
        or identity.startswith("-")
        or any(
            ord(character) < 0x20 or ord(character) == 0x7F
            for character in identity
        )
    ):
        raise WrapperError("daemon wrapper signing identity is invalid")


def compose_wrapper(
    executable_source: Path,
    profile_source: Path,
    signing_identity: str,
    output: Path,
) -> dict[str, Any]:
    require_safe_output(output)
    _validate_signing_identity(signing_identity)
    executable = read_regular(
        executable_source,
        MAX_EXECUTABLE_BYTES,
        "daemon wrapper executable input is invalid",
        executable=True,
    )
    try:
        executable_mode = executable_source.lstat().st_mode
    except OSError as error:
        raise WrapperError("daemon wrapper executable input is invalid") from error
    if executable_source.name != BUNDLE_EXECUTABLE or not executable_mode & 0o111:
        raise WrapperError("daemon wrapper executable input is invalid")
    if executable[:4] not in MACHO_MAGICS:
        raise WrapperError("daemon wrapper executable input is invalid")
    profile = read_regular(
        profile_source,
        MAX_PROFILE_BYTES,
        "daemon wrapper profile input is invalid",
        executable=False,
    )
    info = read_regular(
        INFO_PLIST_SOURCE,
        MAX_PLIST_BYTES,
        "daemon wrapper Info.plist authority is invalid",
        executable=False,
    )
    entitlements = read_regular(
        ENTITLEMENTS_SOURCE,
        MAX_PLIST_BYTES,
        "daemon wrapper entitlement authority is invalid",
        executable=False,
    )
    if load_plist(entitlements) != EXPECTED_ENTITLEMENTS:
        raise WrapperError("daemon wrapper entitlement authority differs")

    candidate_container = output.parent / f".decodexd-wrapper-{uuid.uuid4().hex}"
    candidate = candidate_container / WRAPPER_NAME
    candidate_container_created = False
    output_created = False
    published = False
    try:
        candidate_container.mkdir(mode=0o700)
        candidate_container_created = True
        candidate.mkdir(mode=0o700)
        contents = candidate / "Contents"
        macos = contents / "MacOS"
        contents.mkdir(mode=0o755)
        macos.mkdir(mode=0o755)
        write_exclusive(contents / "Info.plist", info, 0o644)
        write_exclusive(contents / "embedded.provisionprofile", profile, 0o644)
        write_exclusive(macos / "decodexd", executable, 0o755)
        run_tool(
            [
                str(CODESIGN),
                "--force",
                "--sign",
                signing_identity,
                "--options",
                "runtime",
                "--timestamp=none",
                "--entitlements",
                str(ENTITLEMENTS_SOURCE),
                str(candidate),
            ]
        )
        inspect_wrapper(candidate)
        if output.exists() or output.is_symlink():
            raise WrapperError("daemon wrapper output already exists")
        os.rename(candidate, output)
        output_created = True
        candidate_container.rmdir()
        directory_flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
        directory_descriptor = os.open(output.parent, directory_flags)
        try:
            os.fsync(directory_descriptor)
        finally:
            os.close(directory_descriptor)
        descriptor = inspect_wrapper(output)
        published = True
        return descriptor
    except WrapperError:
        raise
    except OSError as error:
        raise WrapperError("daemon wrapper output could not be created") from error
    finally:
        if (
            candidate_container_created
            and not published
            and candidate_container.exists()
        ):
            shutil.rmtree(candidate_container)
        if output_created and not published and output.exists():
            shutil.rmtree(output)


def finite_result(descriptor: dict[str, Any]) -> dict[str, Any]:
    return {
        "schema": RESULT_SCHEMA,
        "descriptor": descriptor,
        "descriptor_sha256": canonical_sha256(descriptor),
    }


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = _ArgumentParser(add_help=True, allow_abbrev=False)
    subparsers = parser.add_subparsers(dest="command", required=True)
    compose = subparsers.add_parser(
        "compose",
        add_help=True,
        allow_abbrev=False,
    )
    compose.add_argument("--decodexd", required=True)
    compose.add_argument("--profile", required=True)
    compose.add_argument("--signing-identity", required=True)
    compose.add_argument("--output", required=True)
    inspect = subparsers.add_parser(
        "inspect",
        add_help=True,
        allow_abbrev=False,
    )
    inspect.add_argument("--wrapper", required=True)
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    try:
        args = parse_args(argv)
        if args.command == "compose":
            descriptor = compose_wrapper(
                Path(args.decodexd),
                Path(args.profile),
                args.signing_identity,
                Path(args.output),
            )
        elif args.command == "inspect":
            descriptor = inspect_wrapper(Path(args.wrapper))
        else:
            raise WrapperError("daemon wrapper command is invalid")
        sys.stdout.buffer.write(canonical_json(finite_result(descriptor)) + b"\n")
        return 0
    except WrapperError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
