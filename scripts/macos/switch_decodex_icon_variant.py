#!/usr/bin/env python3
"""Install one prepared, signed Decodex icon trial and reopen the app."""
import json
import os
from pathlib import Path
import plistlib
import signal
import subprocess
import sys
import time
import uuid


def run(*args):
    subprocess.run([str(arg) for arg in args], check=True)


def app_pids():
    listing = subprocess.check_output(["ps", "-axo", "pid=,comm="], text=True)
    paths = {"/Applications/Decodex.app/Contents/MacOS/decodex-gpui", "/Applications/Decodex.app/Contents/Helpers/decodex"}
    return [(int(parts[0]), parts[1]) for line in listing.splitlines()
            if len(parts := line.strip().split(None, 1)) == 2 and parts[1] in paths]


def main():
    root = Path(__file__).resolve().parent
    manifest = json.loads((root / "manifest.json").read_text())
    if len(sys.argv) != 2 or sys.argv[1] not in manifest:
        raise SystemExit("Choose 01-mercury-cloud, 02-flat-cloud, 03-open-cloud, or original")
    name = sys.argv[1]
    source = Path(manifest[name]).resolve()
    if not source.is_relative_to(root / "bundles"):
        raise SystemExit("Trial bundle must remain inside the trial directory")
    info = plistlib.loads((source / "Contents/Info.plist").read_bytes())
    if info.get("CFBundleIdentifier") != "box.acg.decodex":
        raise SystemExit("Unexpected app identity")
    run("codesign", "--verify", "--deep", "--strict", source)
    staged = Path("/Applications") / f".Decodex-icon-{uuid.uuid4()}.app"
    installed = Path("/Applications/Decodex.app")
    rollback = root / "last-installed.app"
    if rollback.exists():
        raise SystemExit("A previous switch needs recovery: last-installed.app exists")
    run("ditto", source, staged)
    run("codesign", "--verify", "--deep", "--strict", staged)
    for pid, path in app_pids():
        if path.endswith("/MacOS/decodex-gpui"):
            try:
                os.kill(pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
    deadline = time.monotonic() + 15
    while app_pids() and time.monotonic() < deadline:
        time.sleep(0.2)
    if app_pids():
        raise SystemExit(f"Decodex did not close. No app was replaced. Prepared bundle: {staged}")
    installed.rename(rollback)
    try:
        staged.rename(installed)
        run("codesign", "--verify", "--deep", "--strict", installed)
    except BaseException:
        if installed.exists():
            installed.rename(root / f"failed-install-{uuid.uuid4()}.app")
        rollback.rename(installed)
        raise
    # Retain rollback bundles without presenting duplicate apps to Launch Services.
    history = root / "backups"
    history.mkdir(exist_ok=True)
    rollback.rename(history / f"Decodex-{uuid.uuid4()}.app")
    run("/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister", "-f", installed)
    os.utime(installed, None)
    subprocess.run(["killall", "Dock"], check=False)
    run("open", "-n", installed)
    print(f"Installed {name}. Dock and menu bar now use this variant.")


if __name__ == "__main__":
    main()
