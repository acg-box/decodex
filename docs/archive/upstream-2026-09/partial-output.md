> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Unfinished answer and plan retention

Upstream commit `529bcb2fdf11025b17709dc88b05d055626646a6`, contained in fixed
cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, flushes TUI answer and plan
streams before terminal handling. Decodex uses its existing native transport and
local display storage; it does not copy the TUI implementation.

## Current behavior

Save unfinished answer and plan text as resolved `partial_output` inbox records
with the terminal receipt. Preserve exact thread, turn, item, kind, text and
truncation state within the existing byte limits. These are display records;
they cannot wake a manager or become completed worker-result evidence.

PR1511 retains saved fallback text after a late completion notification until
complete matching native history is available. A complete, non-truncated native
item with the same source identity takes display precedence. An unrelated turn,
item, thread or incomplete native item cannot hide the fallback. Native history
invalidation clears saved and live output under the existing process-owner check.

The desktop labels the text unfinished, offers source copy and pages older local
records independently of native history. The preserved earlier document's claim
that every late completion removes the fallback is superseded by PR1511.

## Current native verification

Restore the original `native_partial_output.rs` fixture exactly and run it against
installed Codex `0.158.0-alpha.2`. Both answer and plan cases pass. Each case uses
a private native home, a temporary local store and a loopback Responses service
that leaves its stream unfinished until the exact turn is interrupted.

The test observes the real native delta, sends one explicit interrupt, consumes
the terminal event, and confirms that native paginated history omits the unfinished
item. A reopened local store still renders the exact partial source. Each case
makes one inference request; history readback causes no additional inference.
The native child is shut down before the fixture ends. Runtime all-feature,
all-target strict lint passes. No production source change is needed.

This verifies the installed native process, notification handler and store
readback. It is not production-account inference or a full application restart.
The existing persistence and rendered regressions have separate delivery evidence.

## Remaining acceptance

The signed service-to-desktop composition, older-record button, copy interaction
and refresh of cached older pages still need their shared desktop acceptance.
Math rendering is a separate review item; retaining exact source does not prove
that every unfinished formula renders correctly.
