"""Verify shared native config policy without a provider or account request."""

import hashlib
import json
import os
import tempfile
from pathlib import Path

from native_session import B, Session


CASES = (
    ("unknown_keymap", '[tui.keymap.global]\nopen_transcrip = "ctrl-x"\n', "unknown"),
    ("unknown_profile_tui", '[profiles.work.tui]\ntheme = "dark"\n', "unknown"),
    ("valid_tui", "[tui]\nnotifications = false\n", "valid"),
    ("invalid_known_type", 'model_context_window = "wide"\n', "invalid"),
)


def check(name, config, kind, strict):
    expected_success = kind == "valid" or (kind == "unknown" and not strict)
    with tempfile.TemporaryDirectory(prefix="decodex-config-policy-") as home:
        Path(home, "config.toml").write_text(
            config + '\n[analytics]\nenabled = false\n', encoding="utf-8"
        )
        session = None
        error = None
        try:
            session = Session(home, args=("--strict-config",) if strict else ())
            session.call("config/read", {"includeLayers": False, "cwd": home})
        except RuntimeError as failure:
            error = str(failure)
        finally:
            if session is not None:
                session.close()

        success = error is None
        passed = success == expected_success
        if kind == "unknown" and strict:
            passed = passed and "unknown" in (error or "") and "field" in (error or "")
        if kind == "invalid":
            passed = passed and "invalid type" in (error or "")
        return {
            "case": name,
            "strict": strict,
            "expected_success": expected_success,
            "actual_success": success,
            "passed": passed,
            "error": error,
        }


if __name__ == "__main__":
    results = [check(*case, strict) for case in CASES for strict in (False, True)]
    receipt = {
        "source_commit": os.environ["QUALIFIED_CODEX_SOURCE_COMMIT"],
        "binary_sha256": hashlib.sha256(B.read_bytes()).hexdigest(),
        "cases": results,
        "passed": sum(result["passed"] for result in results),
        "failed": sum(not result["passed"] for result in results),
        "limits": "Isolated config/read only; no real account, provider call or installed app.",
    }
    output = Path(os.environ["DECODEX_QUALIFICATION_OUTPUT"])
    (output / "config_policy.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({key: receipt[key] for key in ("passed", "failed")}))
    assert receipt["failed"] == 0, "Native configuration policy differs from the retained contract"
