> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Agent command publication revisions

The signed desktop at commit 471000a054fa1b4c571e015ba08a4a8fec5ce4ed can
lose its recap panel on the first Generate recap command after Agent start.
A diagnostic build records ApplicationOrder quarantine and PublicationOrder,
then recovery through a replacement snapshot. The provider receives no recap
request in the failing attempt. Raising the window before the command does not
prevent the failure.

ServiceApplication publishes AgentChanged for each accepted command with the
work identity and revision zero. A second command for the same work therefore
violates the retained client's strict entity revision order. The client clears
the recap panel when it replaces the connection; panel cancellation can cancel
the accepted recap before inference starts.

Use one increasing Agent notification revision counter in ServiceApplication.
Keep the work identity and typed payload unchanged. This revision belongs to the
service publication lifetime, not the durable work row. A new publication
instance requires a replacement snapshot before the client can reuse state.
The transport owner serializes command execution and publication. Other event
revisions and client ordering checks remain unchanged. The counter uses constant
space and rejects exhaustion without wrapping.

The regression test publishes repeated notifications for the same work with an
intervening notification for another work. It verifies stable work identity and
strictly increasing revisions for each repeated entity. The previous zero-revision
behavior fails this test; the corrected behavior passes. Signed desktop
verification of the original first-click symptom is recorded below.

Diagnostic evidence: /Users/x/.decodex-active-gui-cpybb08l/desktop.log.
The temporary lifecycle logging was restored and is not part of this change.

The typed Agent command client must accept the publication revision in its success
receipt. It previously required zero and reported a valid positive revision as
protocol_malformed. Keep the receipt, server, command, key and work checks, and
continue to reject a missing revision. The protocol fixture fails on a positive
revision before this correction. Protocol 2.97 prevents old strict-zero clients
from silently accepting a session whose command results they cannot decode.

## Signed desktop verification

Build cc0895c3a440a544d6c414028456abd0b469cc77 with dirty=false using the
repository stage script. Bundle contracts and deep strict signatures pass.
The app-owned fixture /Users/x/.decodex-active-gui-mstskkkr starts a native turn,
completes its initial answer and retains the exact native thread
01a0e38e-72a9-7c72-bb05-164fd11d5b5b. Open Task recap and select Generate recap
exactly once. The panel progresses through Cancel recap to the full summary:
The requested fix was tested; installation is still pending.
No reconnect banner or panel reset appears in the observations. Provider requests
increase from one to two; the second is the structured recap. The observed
service child remains PID 6071. Normal menu Quit exits 0, with two total requests.
See first-click-acceptance.json and provider-observations.json in that fixture.
This is local signed acceptance, not installation, notarization or release.

The final protocol suite passes all 168 tests. Strict runtime and protocol Clippy
passes with all features and targets. Broader draft, background and media
acceptance remains separate; maintenance automation stays paused.
