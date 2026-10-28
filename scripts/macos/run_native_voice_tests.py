#!/usr/bin/env python3
"""Build and sign the real WebRTC tests with a development identity."""

import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BINARY_ID = "decodex-gpui::bin/decodex-gpui"
IDENTIFIER = "box.acg.decodex.tests.native-voice"
TEST_NAMES = {
    f"shell::agent_surface::voice::transport::tests::{name}"
    for name in (
        "native_call_exchanges_audio_captions_mutes_and_releases_buffers",
        "remote_channel_loss_reports_failure_and_releases_buffers",
        "cancellation_releases_audio_before_and_after_the_offer_without_an_answer",
        "cancelled_offer_continuation_cannot_resume_or_block_the_next_call",
    )
}
FILTER = f"binary_id(={BINARY_ID}) and (" + " or ".join(
    f"test(={name})" for name in sorted(TEST_NAMES)
) + ")"
NEXTEST = ["cargo", "+stable", "nextest"]


def run_tests(metadata, identity):
    """Keep the Cargo artifact intact and reuse Nextest's build environment."""
    binary = metadata["rust-binaries"][BINARY_ID]
    directory = Path(metadata["rust-build-meta"]["target-directory"]) / "native-voice-tests"
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "run.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        executable = directory / "decodex-native-voice-tests"
        shutil.copy2(binary["binary-path"], executable)
        subprocess.run(
            ["/usr/bin/codesign", "--force", "--timestamp=none", "--identifier", IDENTIFIER,
             "--sign", identity, str(executable)],
            check=True,
        )
        subprocess.run(["/usr/bin/codesign", "--verify", "--strict", str(executable)], check=True)
        binary["binary-path"] = str(executable)
        with tempfile.TemporaryDirectory(prefix="decodex-native-voice-") as temporary:
            metadata_file = Path(temporary) / "binaries.json"
            metadata_file.write_text(json.dumps(metadata))
            reuse = ["--binaries-metadata", str(metadata_file), "-E", FILTER]
            listing = json.loads(subprocess.check_output(
                NEXTEST + ["list", *reuse, "--message-format", "json"], cwd=ROOT, text=True
            ))
            selected = {
                name
                for suite in listing["rust-suites"].values()
                for name, test in suite["testcases"].items()
                if test["filter-match"]["status"] == "matches" and not test["ignored"]
            }
            if selected != TEST_NAMES:
                raise RuntimeError(f"Native voice tests are missing or ignored: {sorted(TEST_NAMES - selected)}")
            print(f"Running {len(selected)} real WebRTC tests with a fixed development signature.", flush=True)
            return subprocess.run(
                NEXTEST + ["run", *reuse, "--test-threads", "1", "--retries", "0"], cwd=ROOT
            ).returncode


def main():
    identity = os.environ.get("DECODEX_APP_SIGN_IDENTITY", "")
    if sys.platform != "darwin" or not identity or identity == "-":
        print("Use macOS and set DECODEX_APP_SIGN_IDENTITY to an existing development identity.", file=sys.stderr)
        return 2
    metadata = json.loads(subprocess.check_output(
        NEXTEST + ["list", "--locked", "-p", "decodex-gpui", "--bin", "decodex-gpui",
                   "--all-features", "--list-type", "binaries-only", "--message-format", "json"],
        cwd=ROOT, text=True,
    ))
    return run_tests(metadata, identity)


if __name__ == "__main__":
    raise SystemExit(main())
