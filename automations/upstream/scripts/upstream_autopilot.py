#!/usr/bin/env python3
"""Operate the standalone Codex upstream adaptation state machine."""

from __future__ import annotations

from pathlib import Path
import sys

SCRIPT_ROOT = Path(__file__).resolve().parent
if str(SCRIPT_ROOT) not in sys.path:
    sys.path.insert(0, str(SCRIPT_ROOT))

from upstream_autopilot_lib import *  # noqa: F403
from upstream_autopilot_lib import cli as cli_module
from upstream_autopilot_lib import core as core_module
from upstream_autopilot_lib import effects as effects_module
from upstream_autopilot_lib import handoff as handoff_module
from upstream_autopilot_lib import observation as observation_module
from upstream_autopilot_lib import state as state_module
from upstream_autopilot_lib import validation as validation_module
from upstream_autopilot_lib.cli import execute, main, parse_args, result_payload


if __name__ == "__main__":
    raise SystemExit(main())
