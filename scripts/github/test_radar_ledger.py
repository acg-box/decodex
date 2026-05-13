from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().with_name("radar_ledger.py")
MODULE_SPEC = importlib.util.spec_from_file_location("radar_ledger", MODULE_PATH)
if MODULE_SPEC is None or MODULE_SPEC.loader is None:
    raise RuntimeError(f"Unable to load {MODULE_PATH}")
radar_ledger = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(radar_ledger)


class RadarLedgerTests(unittest.TestCase):
    def write_json(self, path: Path, payload: dict[str, object]) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(payload), encoding="utf-8")

    def test_ingests_existing_bundle_analysis_and_signal(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            bundle_path = root / "artifacts/github/bundles/openai-codex-pr-123.json"
            analysis_path = root / "artifacts/github/analysis/openai-codex-pr-123.analysis.json"
            signal_path = root / "site/src/content/signals/openai-codex-pr-123.json"
            self.write_json(
                bundle_path,
                {
                    "schema": "github_change_bundle/v1",
                    "repo": "openai/codex",
                    "analysis_mode": "pr_first",
                    "default_branch": "main",
                    "primary_pr": {
                        "number": 123,
                        "title": "Add useful behavior",
                        "body": "",
                        "state": "merged",
                        "labels": [],
                        "url": "https://github.com/openai/codex/pull/123",
                    },
                    "commits": [
                        {
                            "sha": "abc1234",
                            "message": "Add useful behavior",
                            "url": "https://github.com/openai/codex/commit/abc1234",
                            "committed_at": "2026-05-13T00:00:00Z",
                        }
                    ],
                    "files": [
                        {
                            "path": "codex-rs/core/src/lib.rs",
                            "status": "modified",
                            "additions": 1,
                            "deletions": 0,
                        }
                    ],
                },
            )
            self.write_json(
                analysis_path,
                {
                    "kind": "capability",
                    "title": "Useful behavior",
                    "summary": "Adds behavior.",
                    "why_it_matters": "It helps users.",
                    "confidence": "confirmed",
                    "impact": "medium",
                    "proof_points": ["PR exists."],
                },
            )
            self.write_json(
                signal_path,
                {
                    "schema": "signal_entry/v1",
                    "slug": "useful-behavior",
                    "lane": "github",
                    "kind": "capability",
                    "title": "Useful behavior",
                    "published_at": "2026-05-13T00:00:00Z",
                    "summary": "Adds behavior.",
                    "why_it_matters": "It helps users.",
                    "confidence": "confirmed",
                    "impact": "medium",
                    "config_flags": [],
                    "caveats": [],
                    "proof_points": ["PR exists."],
                    "source_refs": {
                        "repo": "openai/codex",
                        "pr_url": "https://github.com/openai/codex/pull/123",
                        "commit_urls": ["https://github.com/openai/codex/commit/abc1234"],
                    },
                },
            )

            connection = radar_ledger.connect(root / "radar.sqlite3")
            try:
                payload = radar_ledger.ingest_existing(
                    connection,
                    bundles_dir=root / "artifacts/github/bundles",
                    analysis_dir=root / "artifacts/github/analysis",
                    signals_dir=root / "site/src/content/signals",
                )
            finally:
                connection.close()

        self.assertEqual(payload["upstream_commits"], 1)
        self.assertEqual(payload["radar_reviews"], 2)
        self.assertEqual(payload["artifact_links"], 4)
        self.assertEqual(payload["bundles_ingested"], 1)


if __name__ == "__main__":
    unittest.main()
