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
verification of the original first-click symptom remains required.

Diagnostic evidence: /Users/x/.decodex-active-gui-cpybb08l/desktop.log.
The temporary lifecycle logging was restored and is not part of this change.

The typed Agent command client must accept the publication revision in its success
receipt. It previously required zero and reported a valid positive revision as
protocol_malformed. Keep the receipt, server, command, key and work checks, and
continue to reject a missing revision. The protocol fixture fails on a positive
revision before this correction. Protocol 2.97 prevents old strict-zero clients
from silently accepting a session whose command results they cannot decode.
