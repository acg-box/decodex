#!/usr/bin/env python3
"""Check retained trust-anchor bytes in an exact official WebRTC archive."""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--baseline-library", type=Path)
    parser.add_argument("--baseline-sha256")
    args = parser.parse_args()
    with args.archive.open("rb") as stream:
        archive_digest = hashlib.file_digest(stream, "sha256").hexdigest()
    if archive_digest != args.sha256:
        raise ValueError("The archive does not match the selected official digest")
    with zipfile.ZipFile(args.archive) as archive:
        header = next(n for n in archive.namelist() if n.endswith("/rtc_base/ssl_roots.h"))
        library = next(n for n in archive.namelist() if n.endswith("/lib/libwebrtc.a"))
        header_bytes = archive.read(header)
        library_bytes = archive.read(library)
    baseline = None
    if args.baseline_library:
        baseline = args.baseline_library.read_bytes()
        if hashlib.sha256(baseline).hexdigest() != args.baseline_sha256:
            raise ValueError("The baseline library does not match its qualified digest")
    fixture = json.loads(Path(__file__).with_name("trust_roots.json").read_text())
    results = []
    for root in fixture["roots"]:
        der = base64.b64decode(root["der_base64"], validate=True)
        if hashlib.sha256(der).hexdigest() != root["sha256"]:
            raise ValueError("The public certificate fixture has changed")
        present = der in library_bytes
        expected = root["role"] == "retained-control"
        if present != expected:
            raise ValueError(f"Unexpected compiled trust anchor: {root['subject']}")
        if not expected and root["subject"].encode() in header_bytes:
            raise ValueError("The retired root remains in the distributed header")
        if baseline is not None and der not in baseline:
            raise ValueError("The qualified baseline lacks the positive control")
        results.append({"subject": root["subject"], "present": present})
    print(json.dumps({
        "status": "passed",
        "archive_sha256": archive_digest,
        "header_sha256": hashlib.sha256(header_bytes).hexdigest(),
        "library_sha256": hashlib.sha256(library_bytes).hexdigest(),
        "baseline_positive_control": baseline is not None,
        "roots": results,
        "limit": "Checks distributed header and compiled DER bytes; does not execute TURN/TLS.",
    }, sort_keys=True))


if __name__ == "__main__":
    main()
