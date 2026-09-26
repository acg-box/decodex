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

- Complete signed desktop acceptance for the manual controls delivered in PR1503.
- Add the upstream automatic delay, progress eligibility and opt-out controls.
- Complete end-to-end lost-reply, task-selection and cold desktop acceptance.
  The public native socket command scenario is qualified below.
- Integrate visible voice transcripts. Current history preparation rejects internal
  realtime handoff envelopes instead of treating them as user-visible text. It does
  not yet provide a voice-history recap.

The upstream maintainer remains paused, including after manual completion.

## Manual desktop entry

The task conversation shows a `Task recap` control when it has a native thread.
Opening the control reads the current service state. `Generate recap` is an
explicit action. The desktop polls the state while the request or its displayed
result is current. It never repeats a generation command after a lost reply.
A failed read hides the old text and retries only the query. The user can cancel.

The panel sends cancellation for the exact pending request when it closes, when
the task changes, or when its service/native source changes. Sending new input
also clears the panel. A completed result does not need a cancellation command
when its panel closes. The service remains the authority for result validity.
The UI checks its panel epoch and cancellation state before it displays an update.
A local cancellation message does not assert that native inference has stopped.

The control uses the existing accessible mouse and keyboard button component.
Recap text is plain text; it does not execute links or interpret markup. The
visual fixture is selected with `DECODEX_VISUAL_WORKSPACE_PAGE=recap` in the
repository's workbench capture binary. It uses synthetic text and no account.

This is an optional product control for the final subtraction review. It does
not complete automatic eligibility/delay/opt-out, visible voice transcript
integration, public service command acceptance, or signed desktop acceptance.


## Public native service qualification

The opt-in `installed_recap_public_socket_preserves_parent_and_exact_request_identity`
fixture uses the actual local ProtocolServer, ServiceApplication, ChiefHost and
account-bound native runtime. Credentials and the Responses provider are synthetic.
The child HOME must be a private directory under the user-owned home, outside any
existing `.codex` directory; `/tmp` fails the normal selected-directory policy.
The `.decodex-recap-fixture` marker contains `isolated-recap` and a newline. Set
HOME, CODEX_HOME, DECODEX_TEST_ACCOUNT_HOME and DECODEX_TEST_CODEX_BINARY only in
the test child. Start from a fresh directory. The fixture supplies fresh synthetic
quota facts and shuts down its service even when an assertion fails.

The installed 0.158.0-alpha.2 passed: a cold query makes no model request; a public
Start command creates the parent; GenerateRecap reaches Ready; same-key socket
replay does not infer again; parent latest-turn identity is unchanged; new public
input invalidates the result; stale cancellation cannot cancel the newer request;
exact cancellation removes its result. The separate synthetic desktop socket test
covers a lost command reply. These are distinct from signed desktop acceptance.

This fixture exposed a missing isolation setting. With model metadata selecting
multi-agent v2, `features.multi_agent=false` and `features.multi_agent_v2=false`
do not suppress the `collaboration` namespace. At the fixed upstream endpoint,
`core/src/config/mod.rs::multi_agent_version_override` gives `agents.enabled=false`
precedence over model metadata. The temporary thread now sets that value too.
The same fixture failed on nonempty recap tools before the change and passed
with no tools afterward. It keeps the assertion and never executes those tools.
This changes only the temporary request config, not the parent task's agent policy.

## Voice transcript freshness

A nonempty user or assistant realtime transcript delta/done changes the selected
conversation even when no task turn starts. The transport now invalidates that
thread's existing read-to-write guard before service delivery. A recap query then
hides the old result without causing a cancellation effect. The service routes the
same notification to its normal voice owner and cancels the affected recap.
Empty transcript events and another thread's events do not retire this result.

The regression fixture sends native JSON notifications through the retained
transport. It failed with Ready before the fix and passed with Cancelled after it,
before the service routed the event. It also verifies that the read has no native
cancellation side effect and that routing preserves the voice event for its owner.
This is transport evidence, not microphone or live voice acceptance.

Visible voice-history integration is still open. The existing store owns session
identity, transcript sequence, thread/generation and the pre-call baseline turn.
It does not record an exact native turn for each spoken sentence. An integration
must preserve that partial ordering and must not invent a total order from text
similarity or observation timestamps. Internal realtime delegation envelopes remain
excluded from recap input.
