from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().with_name("backfill_release_range.py")
MODULE_SPEC = importlib.util.spec_from_file_location("backfill_release_range", MODULE_PATH)
if MODULE_SPEC is None or MODULE_SPEC.loader is None:
    raise RuntimeError(f"Unable to load {MODULE_PATH}")
backfill_release_range = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(backfill_release_range)


def release(tag_name: str, prerelease: bool, published_at: str) -> dict[str, object]:
    return {
        "tag_name": tag_name,
        "name": tag_name,
        "prerelease": prerelease,
        "published_at": published_at,
        "url": f"https://github.com/openai/codex/releases/tag/{tag_name}",
    }


def compare(stable_tag: str, preview_tag: str, pr_numbers: list[int]) -> dict[str, object]:
    return {
        "stable_tag_name": stable_tag,
        "prerelease_tag_name": preview_tag,
        "compare": {
            "status": "ahead",
            "ahead_by": len(pr_numbers),
            "total_commits": len(pr_numbers),
            "url": f"https://github.com/openai/codex/compare/{stable_tag}...{preview_tag}",
            "commit_shas": [f"deadbeef{number}" for number in pr_numbers],
            "pr_numbers": pr_numbers,
        },
        "tracked_signal_slugs": [],
    }


class LoadSelectedComparisonTests(unittest.TestCase):
    def write_release_delta(self, path: Path) -> None:
        payload = {
            "schema": "release_delta/v1",
            "repo": "openai/codex",
            "tag_prefix": "rust-v",
            "generated_at": "2026-05-13T00:00:00Z",
            "stable_release": release("rust-v0.130.0", False, "2026-05-01T00:00:00Z"),
            "prerelease": release("rust-v0.131.0-alpha.9", True, "2026-05-12T00:00:00Z"),
            "compare": compare("rust-v0.130.0", "rust-v0.131.0-alpha.9", [22404])["compare"],
            "release_options": {
                "stable": [
                    release("rust-v0.130.0", False, "2026-05-01T00:00:00Z"),
                    release("rust-v0.129.0", False, "2026-04-20T00:00:00Z"),
                ],
                "preview": [
                    release("rust-v0.131.0-alpha.9", True, "2026-05-12T00:00:00Z"),
                    release("rust-v0.131.0-alpha.8", True, "2026-05-11T00:00:00Z"),
                ],
            },
            "comparisons": [
                compare("rust-v0.130.0", "rust-v0.131.0-alpha.9", [22404]),
                compare("rust-v0.129.0", "rust-v0.131.0-alpha.8", [22397]),
            ],
            "tracked_signal_slugs": [],
        }
        path.write_text(json.dumps(payload), encoding="utf-8")

    def test_defaults_to_top_level_stable_and_prerelease(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "release-delta.json"
            self.write_release_delta(path)

            comparison, stable_tag, preview_tag = backfill_release_range.load_selected_comparison(
                path,
                None,
                None,
            )

        self.assertEqual(stable_tag, "rust-v0.130.0")
        self.assertEqual(preview_tag, "rust-v0.131.0-alpha.9")
        self.assertEqual(comparison["compare"]["pr_numbers"], [22404])

    def test_can_select_explicit_stable_and_prerelease(self) -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            path = Path(tmpdir) / "release-delta.json"
            self.write_release_delta(path)

            comparison, stable_tag, preview_tag = backfill_release_range.load_selected_comparison(
                path,
                "rust-v0.129.0",
                "rust-v0.131.0-alpha.8",
            )

        self.assertEqual(stable_tag, "rust-v0.129.0")
        self.assertEqual(preview_tag, "rust-v0.131.0-alpha.8")
        self.assertEqual(comparison["compare"]["pr_numbers"], [22397])


if __name__ == "__main__":
    unittest.main()
