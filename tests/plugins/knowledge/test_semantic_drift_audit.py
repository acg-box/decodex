from __future__ import annotations

import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parents[3] / "scripts" / "semantic-drift"
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from semantic_drift_audit import parse_diff


def test_parse_diff_tracks_doc_claims_and_executable_terms() -> None:
    diff = """diff --git a/docs/example.md b/docs/example.md
--- a/docs/example.md
+++ b/docs/example.md
@@ -1 +1 @@
+The CLI must support `decodex diagnose --json`.
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1 +1 @@
-const OLD_STATUS: &str = "legacy_status";
+const NEW_STATUS: &str = "current_status";
"""

    packet = parse_diff(diff)

    assert packet["changed_docs"] == ["docs/example.md"]
    assert packet["changed_executable"] == ["src/main.rs"]
    assert packet["added_claims"] == [
        {"path": "docs/example.md", "text": "The CLI must support `decodex diagnose --json`."}
    ]
    assert packet["removed_terms"] == ["OLD_STATUS", "legacy_status"]
