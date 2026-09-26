# Task recap integration

Classification: optional product capability. Manual recap, saved voice input and
optional desktop scheduling are implemented. Signed desktop and live voice
acceptance remain open. Automatic scheduling defaults to disabled.

Fixed upstream reference: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Protocol2.84 introduced GenerateRecap, CancelRecap and GetChiefRecap.
Protocol2.85 added the desktop preference; protocol2.86 retains that contract.
The [temporary request owner](temporary-structured-requests.md) runs inference.

## Service ownership

Generation names the exact work and native thread. Database ownership, account,
process generation, native history/settings guards and voice-history revision
must remain current before preparation, inference and publication. A query reads
state; it does not start, replay or cancel inference. Recap commands do not wake
ordinary pending work. One request can be active or cleaning up; up to 32 recent
states remain in memory. Service restart clears them and does not replay them.
Same-key command receipts apply to the current server instance, not across restart.

The selected native model/provider and permissions remain in force. An ephemeral
system thread disables tools, MCP servers, apps, plugins and native agents. The
explicit `agents.enabled=false` setting is required even when both multi-agent
feature flags are false: model metadata can otherwise enable collaboration tools.
The parent thread receives no recap prompt or result. Cancellation attempts exact
interruption and detachment. Local Cancelled state is not proof of immediate native
termination. An uncertain generation command is never replayed.

## Native and voice input

Native history uses complete turn/item pages for paginated threads and bounded
includeTurns for legacy threads. The reader selects recent answered exchanges and
newer unanswered input. It rejects running turns and incomplete or malformed
pages. Reads have a 25-second deadline and an aggregate native frame bound.
Tool output and private reasoning are excluded. Images use placeholders; named
references retain their names. Failed or interrupted turns retain their caveats.
Unpaired public assistant output is retained without inventing a user message.

Voice input reuses chief_voice_calls and resolved chief_inbox_events. It reads
only the exact work/thread, with the voice_transcript source namespace and exact
session identity. Up to eight recent calls and 32 recent recorded sentences per
call are read in one database snapshot. The source marks omissions. No duplicate
transcript store or new database migration is added.

Native task turns and spoken dialogue remain separate sources in the prompt.
Native turn IDs, voice session/sequence and the pre-call native baseline preserve
known provenance. Recording sequence does not prove a total spoken/native order.
Partial captions can be flushed by role when a call closes. The prompt tells the
model to preserve uncertainty when conflicting corrections cannot be ordered and
not to count repeated spoken/native wording as additional completed work.

New records distinguish complete transcript events from partial closing captions.
Old records retain unknown completeness; there is no backfill that guesses it.
A selected call without stored text is disclosed as missing transcript evidence,
not evidence that no instruction was spoken. Known voice history permits the
reader to omit internal realtime delegation envelopes while retaining public task
output. Those internal execution instructions are never presented as spoken text.

Both sources share the 32-KiB full-prompt budget. Excerpts retain the beginning and
end of the latest answer and pending correction, with explicit omission markers.
The response requires summary and nullable next_action, with no extra fields;
limits are 700 and 200 Unicode characters.

## Freshness and desktop behavior

Generation waits until the selected voice call is closed. The desktop displays
that known rejection reason. A voice-history revision covers appended transcript
IDs, call count, open-call count and closure state. A changed revision hides cached
results and prevents an old prepared request from publishing.

Nonempty user/assistant realtime transcript delta/done notifications also
invalidate the existing per-thread guard before service event delivery. A read
then hides the old recap without a cancellation effect. The service cancels its
request and passes the same event to the normal voice owner. Empty text and other
threads do not invalidate this result.

The task conversation has a Task recap control for native-backed tasks. Opening
it only reads. Generate is explicit. Lost replies lead to status queries, never a
second generation. Source changes, new input, task changes and panel closure
retire pending work by exact request ID. Completed results survive panel closure
while their source remains current. The UI checks its epoch and cancellation
state before displaying updates. Text is plain; it does not execute markup.

## Evidence

The installed-native fixture uses the real ProtocolServer, ServiceApplication,
ChiefHost and account-bound runtime with synthetic credentials and a local
Responses provider. It verifies that stored voice text reaches the recap request,
that no tools are present, that open voice calls reject generation, and that late
stored captions invalidate prior results without inference from the status query.
It also checks same-key replay counts, unchanged parent history, new input and
exact versus stale cancellation. The fixture runs with Codex 0.158.0-alpha.2.

The test child HOME must be a fresh private directory under the user-owned home,
outside any existing .codex directory. /tmp fails the normal selected-directory
policy. Set HOME, CODEX_HOME, DECODEX_TEST_ACCOUNT_HOME and DECODEX_TEST_CODEX_BINARY
only in the test child. The .decodex-recap-fixture marker contains isolated-recap
and a newline. Test shutdown also runs after assertion failures.

Other tests cover UTF-8 budget/provenance, internal-handoff exclusion, missing
captions, stored complete/partial/legacy metadata, bounds, transport-observed
invalidation, lost desktop command replies and known rejection feedback.
These fixtures do not prove microphone capture or signed desktop acceptance.

## Automatic recap preference foundation

Local protocol 2.85 adds auto_recap to the existing desktop settings readback and
an optional field in SetDesktopSettings. Database migration42 adds the preference
to the existing singleton, with false as the default for both fresh and upgraded
stores. It preserves prior settings and their revision. An omitted command field
preserves the stored choice; an explicit false disables it. The existing optimistic
revision and settings publication apply.

The General settings page exposes Automatic task recaps with a model/quota cost
notice. The existing settings controller sends the explicit choice and waits for
service readback. Keep it disabled during the manual catch-up; daily upstream
maintenance also remains paused.

The fixed upstream TUI policy requires at least three completed turns, then two
new completed turns between recaps. The deadline is 30 minutes after the later
of focus loss and the last finished turn. Focus gain cancels automatic work;
manual generation remains separate from automatic eligibility. A failed automatic
attempt permits at most one 30-second retry for the same turn revision.

Validation covers fresh defaults, version41 upgrade and unchanged migration
checksums, preference isolation, exact revisions, reopen, optional wire fields,
and the service command/result/event/readback path without a native provider.
No production database or user preference was changed by these tests.

## Desktop automatic lifecycle

The existing shell lifecycle poll drives checks; there is no second scheduling
loop. Only the selected Chief task is eligible, while the Chief destination is
selected, the service is online, the setting is enabled and the window is away.
Running work, undisposed events and local voice/dictation keep the quiet period
open. A source or activity change resets the quiet observation. The deadline is
30 minutes after the later observed focus loss or quiet start. These conservative
observations do not reconstruct native wall-clock completion times.

At the deadline, the desktop reads native timeline pages through the existing
service owner. Up to eight pages and 25 seconds can establish the three most
recent distinct successful completed turns. Failed terminal boundaries do not
count; repeated IDs across pages count once. Task/thread and account identities
must remain consistent. Insufficient or unavailable evidence does not start
inference. No history is inserted into the UI or persisted by this reader.

Three completed turns permit the first automatic recap. At least two different
completed turn IDs are required after the previous recap baseline. Automatic
results keep the pre-request baseline; manual results establish a read-only
baseline before automatic generation. Closing the panel does not discard a
pending manual baseline. Native progress after a delayed baseline observation
can require additional work before the next automatic recap.

An eligible check calls the same one-shot generation path as the manual action.
A failed history read or known failed recap permits at most one 30-second retry
for the same observed source/activity version. An uncertain command is still
resolved through read-only polling, never resent. Focus gain, opt-out, leaving
the Chief destination or loss of the selected source cancels pending automatic
work. These automatic controls do not cancel a manual request. Native source
invalidation still applies to both.

The ongoing recap I/O loop uses its own thread so it cannot occupy the shared
GPUI executor indefinitely. The existing watch channel owns cancellation and
normal exit. Generation identity and exact service cancellation remain unchanged.

Desktop tests cover eligibility timing, retry bounds, progress paging and account
changes, setting readback, baseline retention, and the real UI request path over
a same-UID synthetic service. The latter drops the generation reply, confirms one
generation through status and receives exact cancellation on focus gain. It uses
GPUI's documented parking mode for real I/O. These synthetic-service tests do not substitute for signed desktop acceptance.
The installed-native automatic qualification below covers the actual service path.

## Installed-native automatic qualification

Codex 0.158.0-alpha.2 passed the public-socket fixture with the actual GPUI capture
binary. Nine parent turns produced nine distinct completed native turn IDs over
multiple pages. Progress and wrong-binding reads did not add model requests.
The desktop then used its normal automatic driver, with an elapsed fixture clock,
to read native progress and generate one recap. Its Ready state, request ID and
result matched the service query exactly. Native parent history stayed unchanged;
same-key replay and later caption invalidation retained the established behavior.

Set DECODEX_TEST_RECAP_GUI_BINARY to the built
`decodex-gpui-workbench-visual-capture` binary to include this branch of
`installed_recap_public_socket_preserves_parent_and_exact_request_identity`.
Without that variable, the test covers the native service and progress only.
The child capture requires the explicit fixture root and .decodex-recap-fixture
marker. It emits automatic-recap.png, automatic-recap.recap.json, a process ID
and incremental capture diagnostics under that isolated home. Its timeout kills
the child. The preference is enabled and restored only in the fixture database.

This run used the real installed native process and desktop code with a synthetic
local model provider. It did not use a microphone, wait 30 real minutes, change
production preferences or qualify a signed installed Decodex application.

The run exposed [shared-executor occupation by live output observation](chief-output-observation.md).
Moving that existing long-lived I/O loop off the shared executor allowed the
native automatic path to finish. The desktop capture uses GPUI's documented
parking mode for real I/O with deterministic rendering.

## Real service acceptance after a lost desktop reply

Set DECODEX_TEST_RECAP_LOST_REPLY=1 with DECODEX_TEST_RECAP_GUI_BINARY to include a
transport fault in the isolated public-socket fixture. A test-only local proxy forwards
to the original production service. It suppresses the selected generation receipt,
waits for the successful real command result, drops that result and closes the socket.
Subsequent desktop status queries still reach the same service. No second recap or
execution owner is introduced.

The signed desktop capture passed this case. Exactly one generation command reached
the service, one successful result was dropped, and three subsequent status queries
recovered Ready with the original request ID. The enclosing native fixture still
verified the expected model-request count and unchanged parent history. The recap
preference was restored only in the isolated database. Runtime Clippy passed.

The ordinary signed capture also passed and its rendered Ready summary was inspected.
These captures use the desktop's normal driver with a controlled clock. They do not
claim physical foreground/background events, a microphone or a live subscription.

## Normal signed application interaction

On 2026-09-26, the opt-in `DECODEX_TEST_DESKTOP_APP` fixture launched the normal
signed application from an explicit executable path. The bundled helper reports
commit `c9abecb1709629090e68c027f8f359d5f538c7c4`, with `dirty: false`.
The bundle passed `codesign --verify --deep --strict`. The real local service and
installed Codex used a private HOME and synthetic provider. No real subscription
or microphone was used.

The interaction run opened the isolated task, selected Task recap, and selected
Generate recap. The controls returned from Cancel recap to Refresh/Generate.
Command-Q exited the exact child process successfully. The fixture relaunched the
same executable, and the accessibility tree showed the same task and saved native
answer. Opening Task recap did not submit another model request. Quit Decodex from
the native app menu also exited successfully. The fixture recorded two launches,
two successful exits and two total model requests: the initial task and its recap.
The final real-service fixture passed without a provider panic.

The screenshot API returned a blank image, so this run does not prove visual
rendering or recap text visibility. The service remained alive across both GUI
processes; this does not test app-owned service shutdown or a cold service restart.
The harness writes process counters to `desktop-ready.json`; `desktop-relaunch`
and `desktop-finish` are explicit operator markers. A finish marker checks process
exit only and does not certify all manual acceptance steps.

An earlier run exposed a fixture mismatch: the shared provider expected spoken
input in a task without voice. Interactive mode now checks its actual initial
prompt. Provider task panics now fail the enclosing fixture instead of producing
a misleading pass after GUI shutdown. Check exits through the fixture process
readback: an accessibility inspection can reopen an app after it has quit.

## Remaining scope

- Signed desktop foreground/background and opt-out interaction.
- Signed desktop and live voice acceptance, including task selection and cold UI.
- Normal installed application lifecycle acceptance remains shared with R07/R12.

The upstream maintainer remains paused, including after manual completion.
