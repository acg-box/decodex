"""Qualify native sampling through the public service with an explicit runtime package."""

import argparse
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from managed_runtime import qualify


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, required=True)
    args = parser.parse_args()
    qualify(args.runtime,
        "test(installed_recap_public_socket_preserves_parent_and_exact_request_identity)",
        ".decodex-recap-fixture", "isolated-recap\n", {"DECODEX_TEST_NATIVE_SAMPLE": "1"})


if __name__ == "__main__":
    main()
