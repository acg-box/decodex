"""Qualify on-demand startup checks with the exact managed native component."""

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
        "test(installed_native_startup_configuration_preserves_auth_config_and_locked_history)",
        ".decodex-startup-config-fixture", "isolated-native\n", {})


if __name__ == "__main__":
    main()
