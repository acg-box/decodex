#!/usr/bin/env python3
"""Sync Codex config feature toggles from the upstream config.schema.json."""

from __future__ import annotations

import argparse
import html
import json
import re
import urllib.request
from datetime import UTC, datetime
from pathlib import Path
from urllib.parse import quote

SCHEMA_URL = "https://raw.githubusercontent.com/openai/codex/main/codex-rs/core/config.schema.json"
CONFIG_REFERENCE_URL = "https://developers.openai.com/codex/config-reference"
REPO_ROOT = Path(__file__).resolve().parents[4]
DEFAULT_OUT = REPO_ROOT / ".agent/automations/radar/cache/generated/codex-config-features.json"
REFERENCE_DESCRIPTION_OVERRIDES = {
    "features.multi_agent_v2": (
        "Enable MultiAgentV2 collaboration tools (`spawn_agent`, `send_message`, "
        "`followup_task`, `wait_agent`, `close_agent`, and `list_agents`). PR #25636 "
        "renamed the v2 trigger-turn tool from legacy `assign_task` to `followup_task`; "
        "older rollout traces may still mention `assign_task`."
    )
}
REFERENCE_ENTRY_RE = re.compile(
    r"&quot;key&quot;:\[0,&quot;(features\.[^&]+?)&quot;\],"
    r"&quot;type&quot;:\[0,&quot;([^&]+?)&quot;\],"
    r"&quot;description&quot;:\[0,&quot;(.*?)&quot;\]"
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default=SCHEMA_URL, help="Raw upstream config schema URL.")
    parser.add_argument("--out", default=str(DEFAULT_OUT), help="Catalog JSON output path.")
    return parser.parse_args()


def utc_now_iso() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def fetch_json(url: str) -> dict:
    with urllib.request.urlopen(url) as response:
        return json.load(response)


def fetch_text(url: str) -> str:
    with urllib.request.urlopen(url) as response:
        return response.read().decode("utf-8")


def extract_reference_descriptions(raw_html: str) -> dict[str, str]:
    descriptions: dict[str, str] = {}
    for key, _value_type, description in REFERENCE_ENTRY_RE.findall(raw_html):
        descriptions[key] = html.unescape(description)
    return descriptions


def main() -> None:
    args = parse_args()
    schema = fetch_json(args.url)
    reference_descriptions = extract_reference_descriptions(fetch_text(CONFIG_REFERENCE_URL))

    features = (
        schema["definitions"]["ConfigProfile"]["properties"]["features"]["properties"]
    )

    feature_items = [
        {
            "name": name,
            "config_path": f"features.{name}",
            "toml_assignment": f"{name} = true",
            "toml_snippet": f"[features]\n{name} = true",
            "cli_enable_flag": f"--enable {name}",
            "schema_url": args.url,
            "reference_url": CONFIG_REFERENCE_URL,
            "reference_description": REFERENCE_DESCRIPTION_OVERRIDES.get(
                f"features.{name}",
                reference_descriptions.get(f"features.{name}"),
            ),
            "github_search_url": f"https://github.com/openai/codex/search?q={quote(f'\"{name}\"')}&type=code",
        }
        for name in sorted(features)
    ]

    payload = {
        "schema": "codex_config_feature_catalog/v1",
        "source_url": args.url,
        "generated_at": utc_now_iso(),
        "feature_count": len(feature_items),
        "features": feature_items,
    }

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(out)


if __name__ == "__main__":
    main()
