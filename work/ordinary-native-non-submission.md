# Ordinary native non-submission

Classification: core compatibility for existing ordinary conversation input.
Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

## Behavior

The private gateway uses the shared native refusal classifier from PR1488. It
requires the exact request response ID, code -32600 and the complete native
server-draining or managed-provider-change message. An earlier non-warning
notification or server request prevents positive non-submission classification.
A wrong ID, different code, altered message or lost response remains uncertain.

The ordinary coordinator records a positive non-submission receipt with the
native thread ID and no native turn ID. The existing provider-evidence transaction
also fails the exact local user turn and appends one status item. It checks the
active session and thread, streaming history and competing attempts. A local
identity mismatch or history write failure rolls back the transaction. Generic
provider lookup evidence and other consumers retain their existing semantics.

The input remains available. Native acknowledgment and last-known turn evidence
are unchanged. No resend, provider switch or native execution implementation is
added. The coordinator refreshes durable revisions, publishes a history refresh,
and offers manual recovery. It retains the process until an explicit recovery
action; an uncertain failure keeps the existing recovery fence.

## Evidence and limits

Subprocess fixtures cover both native messages, wrong response identity, wrong
code, altered text and prior native activity. Database fixtures cover first-input
and acknowledged-session state, wrong thread, atomic rollback, reopen and receipt
replay without duplicate history. The native policy contract was qualified with
installed Codex 0.155.0-alpha.16.4 in PR1488; that qualification does not establish
ordinary desktop recovery acceptance. No signed desktop interaction is claimed.

The inherited snapshot helper silently accepted a local turn mismatch. This
adaptation rolls back instead. It also publishes current revisions and uses the
shared classifier rather than copying native strings into another owner.

Large approval payloads and optional product scopes remain separate. Scheduled
automation stays paused, including after manual delivery.

Local validation: the full database suite passed 124 tests before the final source
filter and active-session check. The final focused database test passed with both
acknowledgment states. The final full runtime suite passed 570 tests with 41
explicit opt-in tests skipped. Strict database and runtime Clippy passed for all
features and targets. These results do not establish release installation.
