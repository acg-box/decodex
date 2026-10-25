#!/usr/bin/env python3
"""Create local synthetic PCM for the explicit online voice acceptance tests."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import wave

PHRASE = "Please reply with the words blue garden seven."


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    args.directory.mkdir(parents=True, exist_ok=True)
    source = args.directory / "input.aiff"
    subprocess.run(
        ["/usr/bin/say", "-v", "Samantha (English (US))", "-r", "150", "-o", str(source), PHRASE],
        check=True,
        timeout=30,
    )
    files = {}
    for rate in (24000, 48000):
        target = args.directory / f"input-{rate}.wav"
        subprocess.run(
            ["/usr/bin/afconvert", str(source), str(target), "-f", "WAVE", "-d", f"LEI16@{rate}", "-c", "1"],
            check=True,
            timeout=15,
        )
        with wave.open(str(target), "rb") as stream:
            assert (stream.getnchannels(), stream.getsampwidth(), stream.getframerate()) == (1, 2, rate)
            pcm = stream.readframes(stream.getnframes())
        name = f"input-{rate}.pcm"
        (args.directory / name).write_bytes(pcm)
        files[name] = {"sha256": hashlib.sha256(pcm).hexdigest(), "bytes": len(pcm)}
    (args.directory / "fixture.json").write_text(
        json.dumps({"synthetic": True, "microphone_opened": False, "phrase": PHRASE, "files": files}, indent=2) + "\n"
    )
    print("Prepared synthetic speech at 24 kHz and 48 kHz; no microphone was opened.")


if __name__ == "__main__":
    main()
