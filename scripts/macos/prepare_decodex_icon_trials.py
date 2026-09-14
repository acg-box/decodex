#!/usr/bin/env python3
"""Package native Liquid Glass icon trials against the installed signed app."""
import argparse
import hashlib
import json
from pathlib import Path
import plistlib
import shlex
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
VARIANTS = ("01-mercury-cloud", "02-flat-cloud", "03-open-cloud")
LABELS = ("1 - Mercury Cloud", "2 - Flat Cloud", "3 - Open Cloud")
IDENTITY = "4EBCADF6B4D513E45CE33EC6934C08DBB0F03D7F"


def run(*args):
    subprocess.run([str(arg) for arg in args], check=True, cwd=ROOT)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--base-app", type=Path, default=Path("/Applications/Decodex.app"))
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    original = output / "bundles" / "original" / "Decodex.app"
    if not original.exists():
        original.parent.mkdir(parents=True)
        run("ditto", args.base_app.resolve(), original)
    run("codesign", "--verify", "--deep", "--strict", original)
    run("swift", ROOT / "scripts/assets/build_liquid_glass_icons.swift")
    installer = output / "switch_variant.py"
    shutil.copy2(ROOT / "scripts/macos/switch_decodex_icon_variant.py", installer)
    manifest = {"original": str(original)}
    for slug, label in zip(VARIANTS, LABELS):
        source = ROOT / "assets/app-icon/liquid-glass" / slug
        app = output / "bundles" / slug / "Decodex.app"
        if app.exists():
            raise SystemExit(f"Refusing to overwrite existing trial: {app}")
        app.parent.mkdir(parents=True)
        run("ditto", original, app)
        resources = app / "Contents/Resources"
        run(ROOT / "scripts/macos/compile_decodex_app_icon.sh", resources, source / "AppIcon.icon")
        shutil.copy2(source / "StatusBarIcon.png", resources / "StatusBarIcon.png")
        for filename in ["StatusBarIcon-22.png", "StatusBarIcon-22@2x.png"]:
            shutil.copy2(source / filename, resources / filename)
        info = app / "Contents/Info.plist"
        metadata = plistlib.loads(info.read_bytes())
        metadata["CFBundleIconName"] = "AppIcon"
        metadata["DecodexIconVariant"] = slug
        metadata["DecodexIconBuildKind"] = "native-liquid-glass"
        revision = hashlib.sha256((resources / "Assets.car").read_bytes()).hexdigest()
        metadata["DecodexIconRevision"] = revision
        # IconServices may retain an old layered rendition when a replacement
        # app reuses the same bundle id and build number. Give each icon revision
        # a distinct trial build without changing the core app version.
        metadata["CFBundleVersion"] = f"5.{VARIANTS.index(slug) + 1}.{1000 + int(revision[:8], 16) % 9000}"
        info.write_bytes(plistlib.dumps(metadata))
        run("codesign", "--force", "--options", "runtime", "--timestamp=none", "--sign", IDENTITY, app)
        run("codesign", "--verify", "--deep", "--strict", app)
        manifest[slug] = str(app)
        launcher = output / f"{label}.command"
        launcher.write_text("#!/bin/zsh\nset -eu\nexec /usr/bin/python3 " + shlex.quote(str(installer)) + " " + shlex.quote(slug) + "\n")
        launcher.chmod(0o755)
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    restore = output / "Restore Previous Icon.command"
    restore.write_text("#!/bin/zsh\nset -eu\nexec /usr/bin/python3 " + shlex.quote(str(installer)) + " original\n")
    restore.chmod(0o755)
    print(output)


if __name__ == "__main__":
    main()
