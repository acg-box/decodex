# Subscription voice

## Product behavior

Live voice uses the current Chief and the existing Codex subscription. The composer
has one Live control, live captions, microphone mute, and End. End stops audio; it
does not cancel work that the user already requested. Navigation closes capture.
The existing interrupt control remains the way to stop agent work.

Dictation is a separate requirement: progressive text in an editable draft, final
correction, explicit Send, and Cancel that restores the previous draft. The user
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

## Subscription dictation blocker

The installed app-server schema has no standalone dictation method. The official
desktop implements a separate `/codex/dictation-stream-connect-info` bridge and
connects to `wss://chatgpt.com/backend-api/dictation/stream`. Its subprotocols use the
native auth token. `session.start` selects PCM16 and `streaming_sse`, with segment
or final-only transcript delivery. `audio.append` sends chunks; `session.close`
flushes. Utterance IDs and revisions associate partial and final text.

Standalone reproduction of this observed path received HTTP 403 with
`cf-mitigated: challenge` and an HTML browser-verification page before the dictation
protocol began. Final-only `/transcribe` received the same response. This is a
request-environment/access blocker, not evidence about the user's subscription.
The desktop also has its own network, device and integrity context. Do not extract
its private cookies, bypass challenges, or claim that a header guess resolves it.

A conversational V3 session produced progressive recognition but a silent prompt
did not reliably produce final correction. It can also invoke the backing agent.
Therefore this is not an acceptable replacement for send-later dictation.

The user declined macOS recognition on 2026-09-17. Dedicated subscription dictation
remains incomplete until its authenticated standalone connection is verified.
