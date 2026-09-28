> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Subscription voice

Current status: optional product capability. The implementation retains an editable
composer during Live and call-bound captions after local media retirement. Fresh
signed desktop, physical audio and usable live WebRTC acceptance remain open.
Historical successful probes below do not qualify the current artifact. See
[the current reconciliation](prompt-voice-record-reconciliation.md).

The source versions and validation results below are historical unless explicitly
identified as current. Later restoration supersedes the older Live-hides-editor
behavior. Native Codex owns authentication and voice execution; Decodex owns its
media bridge, draft and presentation.

## Product behavior

Live voice uses the current Chief and the existing Codex subscription. The composer
has one Live control, live captions, microphone mute, and End. End stops audio; it
does not cancel work that the user already requested. Navigation closes capture.
The existing interrupt control remains the way to stop agent work.

Dictation is a separate composer mode: progressive text in an editable draft, final
correction, explicit Send, and Cancel that restores the previous draft. Done stops
capture and waits for final correction. A manual edit stops dictation and remains
intact. Navigation closes capture. Neither mode saves audio to disk. The user
requires subscription authentication for both modes. Do not substitute system ASR,
require an API key, or present a conversational session as draft-only dictation.

## Native ownership

Codex app-server owns subscription authentication, the existing thread, voice
session setup, voice-to-agent work, and recovery of native turns. Decodex observes
those turns in its existing Chief coordinator. It does not replay speech.

The existing Swift bridge contains a small WebKit media host. It owns microphone
permission, WebRTC, echo cancellation, playback, and device teardown. The document
receives SDP only; account credentials remain in the service. The media view is
attached to the native window. The service queues signaling in memory. SQLite
records only call ownership, observed native turns, and received transcripts.

Final transcript events replace pending text. Native close can arrive before the
last final-text event; retain the received tail in history without sending it back
as a new instruction. Closed calls do not reserve account routing. A live call
must end before switching its account.

## Source baseline

- Installed binary: `codex-cli 0.154.0-alpha.6.2`.
- Official source inspected: `openai/codex` commit
  `b0659c53865dd48b0cd69c454368cea3980017cc`.
- Installed experimental schema: generated with `codex app-server
  generate-json-schema --experimental`, under ignored
  `target/codex-subscription-voice-schema/`.
- Read-only desktop reference: version `26.908.70816`, packaged application source.
  No desktop cookies or credential stores were extracted.

Relevant upstream source:

- `core/src/realtime_conversation.rs`: transport selection, native authentication,
  transcript events, voice delegation, and session teardown.
- `core/src/client.rs::create_realtime_call_with_headers`: provider authentication.
- `app-server/src/request_processors/turn_processor.rs`: thread attachment and
  `thread/realtime/start` submission.
- `app-server/src/bespoke_event_handling.rs`: transcript and session notifications.
- `app-server-protocol/src/protocol/v2/realtime.rs`: installed request contract.
- `app-server/tests/suite/v2/realtime_conversation.rs`: upstream integration cases.
- `voice-host/README.md`: proposed private media helper. The installed app does not
  include that helper, so Decodex uses the existing native bridge and WebKit.

## Verified live path

`thread/realtime/start` uses V3, audio output and WebRTC SDP with the existing Chief
thread. The native core uses the current subscription. Transcription mode over
WebRTC is rejected by native core; transcription over WebSocket requires an API
key. These transport restrictions do not establish subscription ineligibility.

Native XCTest on 2026-09-17 passed with generated speech, the actual Decodex service,
and its existing Chief: SDP exchange, media connection, incremental user text,
assistant reply, final user transcript, stop, and helper exit. No microphone was
recorded for these tests. Tests explicitly attach the media host to a window and
start a real audio graph; an earlier detached/suspended graph sent zero packets.
Caption regression tests cover deltas, final correction and delayed other-speaker
results. No audio is saved by the product.

Opt-in test inputs are `DECODEX_VOICE_SIGNAL_HELPER`,
`DECODEX_VOICE_SERVICE_ROOT`, `DECODEX_VOICE_WORK_ID`, and
`DECODEX_VOICE_SAMPLE`. Run `swift test --package-path
apps/decodex-gpui/menubar --filter VoiceMediaHostTests`. The sample must be generated
or explicitly authorized audio. SDP travels through inherited pipes and is not
printed in test reports.

Remaining acceptance: physical microphone permission, audible reply and natural
interruption in the signed app. A successful synthetic test is not this acceptance.

## Verified subscription dictation

The installed app-server schema has no standalone dictation method. The official
desktop implements a separate `/codex/dictation-stream-connect-info` bridge and
connects to `wss://chatgpt.com/backend-api/dictation/stream`. Its subprotocols use the
native auth token. `session.start` selects PCM16 and `streaming_sse`, with segment
or final-only transcript delivery. `audio.append` sends chunks; `session.close`
flushes. Utterance IDs and revisions associate partial and final text.

The initial Node and headless browser reproductions received HTTP 403 with
`cf-mitigated: challenge` before the dictation protocol began. A native Foundation
URLSession connection to the same endpoint passed HTTP 101 and `session.started`
with the same subscription authentication. No cookies, challenge bypass, separate
API key, or system speech recognition were needed. This identifies a working
network implementation; it does not establish the server's internal rejection rule.

On 2026-09-17, generated speech produced 24 incremental transcript revisions and
one final correction in the direct native qualification. A second qualification
passed through the signed bundled service, protocol 2.25, retained Codex account,
and the production native adapter. It returned provisional text before Finish and
the correct complete final text after Finish. Before/after database counts were
unchanged: 48 inbox events, four work items, and 13 historical Live calls. Dictation
created no agent input or Live call. The qualification report is
`target/voice-qualification/bundled-dictation-report.json`. The opt-in protocol example is
`crates/decodex-protocol/examples/dictation_probe.rs`; it accepts base64 PCM frames
on stdin and never receives authentication material.

`DecodexTransport` is a service-only Swift module in the existing signed native
library. URLSession owns the subscription connection. The service obtains its token
from native `getAuthStatus` and passes it only to that same-process adapter. The
GPUI client receives text and status. One bounded session exists in memory; no new
helper process, database migration, or task dispatcher is added. Account rotation
waits until capture ends. A disconnected client releases its session on expiry.
Audio is never automatically replayed after an uncertain response.

## Capture and draft verification

The media ABI now requires the calling GPUI NSView. This removes dependence on
`NSApp.keyWindow`, which could be absent or refer to another window. An unattached
view is rejected. Capture has a timeout and delayed permission replies cannot
restart a cancelled recording. Live and dictation cannot capture concurrently.

Native tests cover exact-window binding, detached-view rejection, retained mute,
PCM frame capture, flush before End, and the real subscription Live exchange.
GPUI tests cover partial replacement, final correction without Send, Cancel that
restores the prior draft, and preservation of manual edits. Protocol tests check
payload size limits and diagnostic redaction. The signed service dictation test
and the Live subscription test both passed. The composer capture was inspected at
`target/visual-tests/chief-dictation-composer.png` (test fixture, not a live task).

A subsequent UI check reached the same running preview without a restart or code
change. The accessibility tree exposed the composer, microphone controls, and
current state. A direct click started physical capture and reached Listening;
Done reached Final correction and then returned to the draft. Live reached Live,
Mute changed the state to Microphone muted, and End released the call. The service
reported zero open voice calls afterward. Graph close and reopen also updated the
accessibility tree. No extra keyboard activation was needed for these clicks.

The pinned GPUI revision includes the AccessKit macOS adapter, enabled by default;
Decodex does not disable it. The former `cgWindowNotFound` is therefore not evidence
that GPUI lacks macOS accessibility. Its exact transient cause remains unconfirmed.
The apparent white screenshots were later isolated to repeated-image presentation,
not a confirmed application capture failure. A complete native screenshot showed
both the interface and its glass material. Re-emitting the exact saved 65,943-byte
JPEG then displayed only a small changed region. The stored bytes had not changed.
Interpret these repeated-image outputs against the full baseline instead of
reporting their empty regions as an application white screen. This check required
no GPUI, accessibility, transparency, or graphics changes. It does not explain the
separate earlier `cgWindowNotFound` result. Audible playback quality and spoken
physical-microphone transcription still require a speech sample; Listening alone
does not prove them.

A short generated sample was also played through the current system output during
UI dictation. Capture completed without an error but produced no transcript.
System audio inspection reported LG ULTRAGEAR+ over DisplayPort as default output
and Shure MV7 over USB as default input. This acoustic test does not establish that
the microphone received the sample. No volume or device settings were changed.
Actual spoken-input transcription remains unverified; all test capture was ended.

## Unified composer and capture latency

The composer now has one editable draft for text and dictation. Dictation status,
Cancel, and Done use its existing toolbar. Live replaces the editor area with a
microphone waveform and uses that toolbar for Mute and End. Draft text and
attachments remain intact while Live is open; keyboard submission cannot send a
hidden draft. Live captions use normal user/assistant chat formatting and yield
to matching saved messages from the current call. The history follows Live with
interpolated scrolling; manual scrolling away from the bottom pauses following.

The plus menu opens shared input-device choices through its Microphone row. Selection applies
to the next recording in either mode and does not change the system default.
Device discovery does not start capture. WebKit resolves the selected device by
its exposed name after permission is available, and reports an unavailable input
instead of silently sending audio from another device. Selection is currently
retained for this application session.

Dictation previously waited for subscription readiness before opening capture,
used 4,096-sample frames at 24 kHz (about 171 ms), and waited 80 ms between client
exchanges. Capture and connection now start together, bounded early audio waits
for readiness, frames contain 2,048 samples (about 85 ms), and exchange polling
uses 20 ms. Done drains audio before final correction. These are reductions in
local waiting, not a measured guarantee about server transcription latency.
Already-granted microphone permission takes the direct path; media views share
an ephemeral WebKit data store. Live waveform samples are real RMS measurements,
coalesced under network delay and interpolated for display.

Validation includes draft cancellation, audio readiness/drain ordering, input
catalog discovery without capture, native PCM/WebRTC tests, and caption/history
replacement across repeated phrases and roles. Signed-app UI checks listed Shure,
iPhone, and MacBook inputs; selecting Shure allowed capture. Dictation kept the
existing editor, Live hid it and followed the conversation, and End restored the
same draft. No test recording was left active. Subjective recognition speed and
waveform response remain part of user acceptance with actual speech.


## Prepared audio host and responsive capture polling

GPUI prepares one idle WebKit audio host for the current native window, without
requesting microphone permission or capturing audio. Dictation and Live take this
prepared host. Closing a session still destroys its active capture; a subsequent
idle render prepares a fresh host. Input discovery returns its host to the idle slot.

Dictation continues draining native capture events every 20 ms while a subscription
request is pending. Previously the request await also blocked level and readiness
updates, making local capture appear stalled behind the handshake. Early PCM remains
buffered until the subscription is ready. Stop and draft-preservation rules remain.

Native tests verify that preparation does not request capture. This run measured
54.6 ms for host preparation and 20.0 ms for warm synthetic capture readiness; these
are local synthetic timings, not physical microphone or subscription latency.
Six native media tests passed; the opt-in subscription qualification was skipped.
The GPUI pending-request regression proves capture-state processing runs before a
network response. The full GPUI suite passed: 173 tests, five opt-in tests ignored.

## Direct microphone capture for subscription dictation

Dictation now captures audio through AVAudioEngine instead of waiting for WebKit
getUserMedia. It still sends mono 24 kHz PCM to the existing subscription ASR
connection. Live remains on its existing WebRTC route. Native input selection
uses the selected device name; an unavailable device produces an error.

The audio tap is explicitly Sendable, so Swift does not require the main executor
on the audio callback. UI events return to the main queue. Conversion drains the
resampler on finish, including its partial final frame. Capture stops on finish,
cancel, and host close. Startup diagnostics record only elapsed milliseconds.

Validation on the signed preview: physical microphone first-buffer readiness was
502 ms on the first attempt and 314 ms on the next attempt. These measurements
exclude subscription recognition and final-correction latency. Finish returned
from final correction to the empty composer; cancel restored the empty draft.
One preview process remained, with no active recording.

Seven native tests passed; the separate opt-in subscription qualification test was
skipped. The new PCM test verifies stereo 48 kHz to mono 24 kHz conversion, bounded
chunks, and the complete final audio duration with a small resampling filter tail.
The GPUI suite passed 173 tests (five opt-in cases ignored); strict Clippy passed.

## Restored call-bound captions and editable drafts

The current [caption recovery](voice-caption-recovery.md) supersedes the older
Live-hides-composer behavior above. Restore the inherited visible editable draft,
per-call transcript receipts, both speakers and local audio cleanup after service
loss. Targeted and rendered tests are recorded separately from the outstanding
physical audio and signed application acceptance.

## Retained historical source review

The following original record preserves applicability decisions and unresolved
caption identity limits. Protocol versions, test counts and WIP labels describe
that historical stage. Current restoration is recorded above and in
[caption recovery](voice-caption-recovery.md). No historical probe is new acceptance.

## Voice source review: commits 1198-1202

The fixed cutoff remains `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

- `3f59eb965a8bed447065dfcc1b5047f987a647bc` enables the TUI voice flag by
  default. Installed Codex reports it as stable and enabled. Decodex does not
  gate its app-server voice control on that TUI flag.
- `ce7fbb373b14b37a5d163735c395e355e272d618` adds signed Windows voice resources
  and Windows system TLS validation. Custom CA selection still takes precedence.
  The platform TLS and package discovery changes are Windows-only and unchanged
  at the cutoff. Decodex currently has a macOS native launch owner and WebKit
  media transport; no Windows runtime or TLS implementation is copied.
- `7ef70f95d5c07976f3c992e413a02a6db238b0d0` refreshes CPAL speaker configuration
  when Bluetooth input changes the output rate. It preserves capture state and
  discards old render references. The cutoff code is unchanged. WebKit owns live
  speaker format negotiation here; Decodex has no cached CPAL speaker stream.
  Real Bluetooth device switching remains part of audio acceptance.
- `1b5e27c7f0e94e395aed438a0b361d0dca07230e` stops idle TUI playback from waiting
  for captions. Decodex already attaches and plays incoming tracks independently
  of caption events. The new WebKit test
  `testIncomingAudioStartsBeforeCaptionsAndRejectsOldCallTracks` passes in
  `/tmp/decodex-1201-media.log`: a track before any caption requests playback,
  replacement invalidates the old peer callback, and the new peer can play.
  It observes playback requests with synthetic tracks. It does not prove audible
  output, usable ICE, microphone permission or a signed app call.
- `16491f7f708d0869fc2356f12172f0b5e3922d19` retains recent meter activity through
  quiet samples. Decodex already removes only the oldest sample and appends the
  new sample; a zero sample does not erase the history. Its live meter displays
  microphone activity, not the TUI's separate speaker meter.


## Upstream 1216: caption handoff (in progress)

Source: `944d6fd1ba4baab69dbedd205282dc72ec20abb5`. The final cutoff keeps the
same realtime caption owner. It retains both speakers and pending completed
captions until history insertion. The later renderer delta only removes unrelated
token activity output.

Decodex had one `VoiceUi` caption. A new speaker replaced the old one and the
regression test expected a late user final to be ignored. The current WIP stores
captions by turn, accepts interleaved and late finals, clears explicit empty finals,
and uses the runtime's 32 KiB UTF-8 boundary. All local media retirement paths
transfer visible text to a separate, work-scoped caption collection. Media and
microphone resources still retire immediately. History matching consumes each
receipt once, so one saved message does not hide repeated captions.

Validation: interleaved/late/empty final unit test passes in
`/tmp/decodex-1216-captions-build.log`. GPUI history/control-loss regression passes
in `/tmp/decodex-1216-caption-handoff.log`: wrong work is hidden, repeated text
requires separate receipts, old session controls cannot stop the current call,
and disconnected media retains captions. Strict GPUI Clippy passes in
`/tmp/decodex-1216-ui-lint.log`. These are not signed desktop or audible tests.

Protocol 2.63 now adds optional `voice_session_id` to each history receipt.
Runtime derives it from the existing durable `["voice_transcript", session,
sequence]` identity only for voice events; malformed identities do not qualify.
Older receipts remain decodable and serialize without the new optional field.
The UI requires matching work, call, voice category, role, text and resolved state.
Each receipt is consumed once. Accepted completed text is removed permanently;
only small consumed receipt IDs remain for the active call. Empty retired calls
are removed. History arrival, caption polling and local retirement reconcile the
same state. Completed caption order is stable; live user text precedes the reply.

Additional checks pass:

- Three GPUI voice tests: `/tmp/decodex-1216-ui-final.log`.
- Three runtime history projection tests, including bounded voice identity and
  rejection of non-voice/malformed source IDs:
  `/tmp/decodex-1216-receipts-runtime.log`.
- Protocol 127 unit and 6 local transport tests:
  `/tmp/decodex-1216-protocol.log` (before the additional optional-field roundtrip).
- Strict protocol/runtime/GPUI all-target Clippy:
  `/tmp/decodex-1216-receipts-lint.log` (before the final cached-history retirement
  reconciliation and wire test).

The optional-field legacy/new roundtrip passes in
`/tmp/decodex-1216-wire-compat.log`; final strict all-target Clippy passes in
`/tmp/decodex-1216-final-lint.log`.

Remaining acceptance: verify rendered order, history delays and switch-away/back in the actual
signed desktop. Stop-time partial text can differ from a later corrected server
final. Current exact-text reconciliation deliberately leaves that unmatched;
this correction case needs a verified identity/ordering solution before declaring
handoff complete. Do not weaken it to arbitrary same-role or prefix matching.
Runtime persistence remains the existing `chief/voice.rs` owner.


### Frameless v3 caption input

The official cutoff parser `codex-api/src/endpoint/realtime_websocket/
protocol_frameless_bidi.rs` consumes `input_transcript.added` and
`output_transcript.added` through `item.text`. `turn.done` requires role and
transcript, but not an ID. The macOS WebKit host already forwards these events;
the old Rust UI discarded them because it required `turn.created` and a turn ID.

The UI now accepts these native forms, retains separate active speakers, applies
role-only finals, binds a final-only ID to its anonymous active caption, and
accepts nonempty finals without earlier deltas. Known turn IDs still route late
finals to their own captions. Empty finals clear only their active speaker; the
32 KiB UTF-8 limit also covers the new input paths.

This does not resolve the stop-time correction identity question. Native timeline
segment IDs are independently generated UUIDs (`core/src/realtime_history.rs`),
not WebRTC turn IDs. Its `finish_segment` seals accumulated deltas when a segment
exists, so do not assume the canonical segment equals every corrected flat final.
The existing native timeline qualifier uses v2 WebSocket speech and cannot prove
v3 WebRTC-to-desktop handoff. Do not add another transcript persistence owner.


### Closing the already queued caption gap

The GPUI poll loop runs at 100 ms intervals. Every retirement path now drains the
native host's current mailbox for caption events before releasing the media object.
The host bounds that mailbox at 128 events, so retirement consumes at most 128 and
never waits for future audio/network input. This retains an already queued corrected
final instead of freezing its previous partial at close.

Five GPUI voice tests pass in `/tmp/decodex-1216-retirement-queue.log`; final strict
Clippy passes in `/tmp/decodex-1216-retirement-lint.log`. A real WKWebView test passes
in `/tmp/decodex-1216-webkit-caption-order.log`: the production data-channel callback
and script-message bridge queue input, reply and corrected final before ended;
a callback from the retired channel cannot enqueue another caption. It uses
synthetic audio and directly invokes the native channel callback, so it does not
prove ICE, remote media delivery or audible playback.

The demonstrated queue-loss bug is fixed. A final received only by the remote
native bridge after local media closure remains a combined-call qualification
case; no exact cross-stream item identity has been established. Keep that case in
final native/desktop acceptance, and reproduce it before introducing a new owner
or loosening receipt matching. Source review of 1216 is complete; full delivery
and signed desktop acceptance are not.
