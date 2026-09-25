# Task recap integration

Classification: optional product capability. The complete feature is not delivered.
This batch adds native-history preparation and service request state on top of
[the temporary request lifecycle](temporary-structured-requests.md).

Reference endpoint: 595cc91e8cbb1c2ca822d0311dcf12709410c582. The implementation
uses the upstream recap_history selection, recap_prompt instructions and structured
result contract. Local protocol 2.84 adds GenerateRecap, CancelRecap and GetChiefRecap.

## Current service behavior

An explicit generation command identifies the local work and its exact native
thread. The existing database ownership check binds it to the active process
generation before admission and before publication. The existing Chief command/publication owner handles acceptance and duplicate
commands in its current server instance. A query reads status; it does not start,
replay or interrupt inference. Recap commands do not wake ordinary pending work.
No new database table, durable job or automatic restart replay is added. A service
restart clears transient recap state; it does not resume the old request. This is
not a claim of cross-restart durable command deduplication.

One recap can be preparing, running or cleaning up at a time. Up to 32 recent task
states are retained. The native event receiver routes temporary-thread events to
their bounded channel and leaves normal task events with the existing coordinator.
Cancel commands name the exact request. Old completion callbacks cannot replace a
new request. New user input, native task changes and disconnects invalidate results.
Native history/settings guards remain alive through preparation, inference and
cached presentation, so transport-observed changes also hide a result before the
service event loop consumes their notifications. Existing native history guards
can conservatively invalidate observations when another thread is reverted.

The reader selects eight recent answered exchanges and a newer unanswered request,
including adjacent steering text. It uses native turn headers and complete item
pages for paginated threads, and the native bounded includeTurns read for legacy
threads. It does not pass tool output, reasoning or other item kinds to the model.
Images and references use text placeholders, not image payloads. Failed or
interrupted turns retain an explicit status caveat. A running turn is not eligible.
Source reads have a time and aggregate byte bound; incomplete or malformed history
is an error, not evidence of an empty conversation.

The complete prompt is at most 32 KiB. It drops old whole exchanges before it
excerpts both ends of the newest answer and pending correction. The prompt retains
upstream instructions about the active goal, completed progress, latest corrections
and unresolved validation or availability caveats. Conversation text is data.

The result requires summary and nullable next_action with no extra fields. Limits
are 700 and 200 Unicode characters. Native inference remains in a separate tool-
isolated ephemeral thread with the selected native model/provider and permissions.
The main thread receives no recap prompt or result. Cancellation attempts exact
interruption and detachment; a cancelled local view is not proof of immediate native
termination. No uncertain model request is replayed.

## Evidence and remaining work

The native fixture uses an isolated Codex home and local synthetic Responses server.
It calls the runtime recap owner directly, verifies native history and unchanged
parent turn identity, observes no tools in recap inference, and verifies that a new
parent input invalidates the result. It is not a full desktop or socket-command
acceptance test. The full workspace run passed 2,468 tests with 66 skipped. After the final native
source guard and legacy-history changes, all nine focused recap tests and the
native fixture passed again. The native fixture covers both legacy and paginated
history. Strict Clippy passed for all workspace packages; the final runtime check
covers these later changes. The socket-command and desktop acceptance gaps below
remain open.

The full workspace run also exposed an older configuration-recovery test error:
when a hook reservation won first, the test later omitted its prior receipt ID.
The fixture now exercises both orders and supplies the correct ID. The separate
concurrent-exclusion test remains. Database behavior is unchanged.

Still required for complete recaps:

- Add desktop manual generation, progress, cancellation and plain-text presentation.
- Add the upstream automatic delay, progress eligibility and opt-out controls.
- Verify actual public socket commands, lost replies, task selection changes and
  cold desktop state, then run signed desktop acceptance.
- Integrate visible voice transcripts. Current history preparation rejects internal
  realtime handoff envelopes instead of treating them as user-visible text. It does
  not yet provide a voice-history recap.

The upstream maintainer remains paused, including after manual completion.
