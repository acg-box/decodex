# Subscription voice

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
