# Streaming voice input investigation

## Intended interaction

Voice input creates an editable composer draft. It does not start a Chief turn,
steer an existing turn, or open an agent voice conversation. Show partial text as
speech arrives. Replace provisional text with the final transcript on completion.
Do not automatically send. Cancel restores the draft from before dictation.
Keep capture/session ownership tied to the composer manager so late results cannot
modify another conversation. Stop capture when navigating away or closing the app.

The microphone control belongs beside Send. During capture, show a small live
level indicator, elapsed time, Done, and Cancel inside the composer. Keep the
existing glass, typography, and control sizing. Avoid a separate recording modal.
Do not save audio by default. Recording starts only after an explicit user action.

## Source evidence (2026-09-16)

- Existing official reference checkout: fd346b8dbaa24573a0244bc917811849d27c4cf4.
- Latest fetched official main: fc2ea82e7eff22c618a56db29c68a6b1967cba7d.
  The reference checkout was not changed. No upstream setup scripts were run.
- Installed binary: codex-cli 0.154.0-alpha.6.2.
- Generated installed schema with `codex app-server generate-json-schema
  --experimental --out target/codex-voice-schema`.
- app-server-protocol/src/protocol/v2/realtime.rs defines start, appendAudio,
  appendText, appendSpeech, stop, and transcript delta/done notifications.
  These methods support the realtime conversation lifecycle. The deeper config
  layer also supports experimental realtime.type=transcription, mapped to
  RealtimeSessionMode::Transcription. Text output modality alone is not enough
  to select that mode. The WebSocket startup path calls realtime_api_key and
  fails without an API credential. This is a reusable base, not a complete
  draft-only composer dictation implementation.
- app-server/tests/suite/v2/realtime_conversation.rs tests transcript events and
  realtime session configuration with background_agent and remain_silent tools.
- Latest main has an in_app_dictation feature flag but the searched open-source
  implementation does not expose a separate standalone dictation request.
  core/src/config/config_tests.rs tests type=transcription;
  core/src/realtime_conversation.rs implements the session-mode mapping and
  API-key requirement for WebSocket transport.
- Installed schemas contain the experimental conversation methods. Schema presence
  does not establish server entitlement or a successful audio session.

## Official API option

https://developers.openai.com/api/docs/guides/realtime-transcription

The documented dedicated session has type=transcription. The current guide uses
`gpt-live-transcribe`, PCM16 at 24 kHz, input_audio_buffer.append, explicit
input_audio_buffer.commit, and transcription delta/completed events. Match events
by item_id; a completed transcript replaces provisional text. A final result does
not imply that the client must upload the full recording a second time.

Use turn_detection=null for user-controlled completion. This transcription-only
session does not generate assistant responses or execute agent tools. Authenticate
through a separately supported API credential path; do not repurpose a ChatGPT
session token or assume Codex account entitlement covers this API.

The exact ChatGPT iOS and desktop dictation implementation is not established by
these public sources. UI behavior alone does not identify its model or transport.

## Earlier investigation

Personal Infisical value-free discovery did not find a declared OpenAI API key.
The user has been asked to choose an independently configured OpenAI streaming
transcription backend or an initial macOS system recognition backend. No secret
values were retrieved. No microphone recording or audio upload has occurred.
No voice capability was implemented during that investigation. The native-first
decision below supersedes the earlier backend-choice question.

## macOS alternative

Apple SpeechAnalyzer and SpeechTranscriber expose progressive transcription with
volatile results and finalized results. This can provide on-device live drafts
without an OpenAI API credential, subject to supported locales and installed
speech assets. It is a different recognizer, not a claim of ChatGPT-equivalent
accuracy. Relevant Apple sources:

- https://developer.apple.com/documentation/speech/speechtranscriber/preset
- https://developer.apple.com/videos/play/wwdc2025/277/

Implementation must measure real microphone latency and mixed Chinese/English
recognition before claiming a quality or speed improvement over ChatGPT.


## Native-first decision and recheck

The user prefers the Codex-native subscription path. Do not require a separate API
key or select macOS recognition as the default before checking that path.

Fetched official main again: `8452164c761c9225b2ee12c2bd1d48f818573704`.
Installed Codex remains `0.154.0-alpha.6.2`; the reference checkout was not changed.

Official desktop documentation confirms both dictation into an editable composer
and live voice for eligible subscriptions. It does not document a third-party
subscription dictation endpoint or its quota accounting:

- https://learn.chatgpt.com/docs/prompting#use-voice-dictation
- https://learn.chatgpt.com/docs/features/voice

Do not generalize the WebSocket API-key restriction to all native voice:
`core/src/client.rs::create_realtime_call_with_headers` uses the configured provider
and its current authentication for WebRTC call creation. However,
`validate_avas_webrtc_start` rejects transcription mode: this transport currently
requires conversational realtime. The transcription-capable WebSocket branch still
calls `realtime_api_key`, with an explicit API-key requirement. Latest main still
exposes the in-app dictation feature flag without a standalone app-server dictation
request in the searched public source. The installed generated request schema also
has no standalone dictation request.

Conclusion: a native voice conversation path exists, but a directly reusable,
subscription-authenticated, draft-only streaming dictation path is not established.
Do not turn speech into automatic agent instructions to imitate dictation. No audio
session, microphone capture, credential extraction, or quota probe was performed.

## Delivery boundary (2026-09-17)

The installed binary and source baseline above remain current for this delivery.
The installed ClientRequest schema has realtime session methods but no standalone
subscription dictation method. The public WebRTC route rejects transcription mode;
the transcription WebSocket path requires a separate API credential. Keep dictation
unavailable rather than turning speech into agent instructions. No microphone button,
recording, audio upload, or independently billed backend was added. Revisit when the
installed native protocol exposes the required draft-only transcription path.

## Subscription voice capability review (2026-09-17)

The user selected subscription authentication. Treat live voice as a separate
deliverable from composer dictation; the dictation limitation does not block an
investigation of native live conversation.

Fetched official main: `b0659c53865dd48b0cd69c454368cea3980017cc`.
The relevant core client, realtime conversation, app-server protocol, and realtime
integration-test files have no changes from the previous reference above.
Installed binary: `codex-cli 0.154.0-alpha.6.2`. Regenerated its experimental schema
under `target/codex-subscription-voice-schema`; it includes V3, WebRTC, transcript
delta/done, audio/text/speech append, stop, and voice-list requests.

### Established source capabilities

- WebRTC call creation uses `current_client_setup(ConfiguredProvider)` and its
  authentication. Codex retains matching authentication for the control channel.
  Do not extract subscription tokens or call an unrelated API directly.
- V3 selects `gpt-live-1-codex`. The upstream WebRTC test checks a live-session
  request and attachment to its sideband without a second session update.
- Realtime events include user and assistant transcript deltas and final text.
  Media travels through the client's WebRTC connection; protocol schemas alone
  do not supply microphone capture, playback, echo cancellation, or device control.
- A session can use context from an existing task and route spoken requests to
  its backing Codex agent. Results can return while the voice session continues.
  Decodex must integrate these turns with Chief's work and dispatch bookkeeping.
- `appendSpeech` supplies text to speak. Supported voices can be queried.
- Native history records transcript segments and promoted agent items. A voice
  session is not a disposable composer draft by default.
- Official desktop documentation describes natural interruption, continued
  conversation during work, task steering, and task coordination. These are
  product capabilities, not evidence that Decodex has implemented them.

### Boundaries

- `clientManagedHandoffs` suppresses automatic Codex-result forwarding to voice.
  It does not disable incoming voice delegations: the core fanout still routes
  `HandoffRequested` into the backing agent.
- WebRTC accepts conversational V1/V3, rejects V2 and transcription mode. Core
  text-only output requires V2. Muting playback is not a transcription-only mode.
- Stopping speech playback, ending a voice session, and interrupting an active
  agent turn are separate operations. Do not label them all as Cancel.
- Plan, rollout, workspace policy, server admission, quota accounting, and actual
  Decodex audio quality remain unverified for this account. No live call, microphone
  capture, audio upload, or account credential read was made for this review.
- The desktop's screen-context and cross-task navigation features require host
  integration; selecting the voice model does not provide those integrations.

Recommended first implementation: an explicit live conversation with the current
Chief, live captions, microphone mute, playback control, and end-call. Reuse the
native subscription call path and preserve native task identity. Keep editable,
send-later dictation as a separate capability until its transport is verified.

Official product source: https://learn.chatgpt.com/docs/features/voice
Source locations: `core/src/client.rs::create_realtime_call_with_headers`,
`core/src/realtime_conversation.rs::prepare_realtime_start`,
`app-server-protocol/src/protocol/v2/realtime.rs`, and
`app-server/tests/suite/v2/realtime_conversation.rs` in `openai/codex`.

## Live qualification and desktop dictation discovery (2026-09-17)

This section supersedes the earlier source-only availability conclusion. The user
confirmed that both dictation and voice conversation work in the official desktop
app. The remaining question is progressive dictation text, not account eligibility.

### Actual subscription tests

Used the installed Codex login through its native app-server, disposable ephemeral
threads, and generated English/Chinese speech. No microphone was captured. No
existing Chief thread received input. Test output is in `target/voice-qualification/`.

| Case | Observed result |
| --- | --- |
| Conversational WebRTC V3 | Connected; data channel open; user transcript deltas and assistant reply arrived. |
| Conversational WebRTC V1 | Rejected: server requires the quicksilver v2 header. |
| Transcription over WebRTC | Rejected by native core: conversational realtime required. |
| Transcription over WebSocket | Rejected by native core: API key authentication required. |
| V3 with a silent dictation prompt | Incremental user text arrived; no backing Codex turn started in these cases. This is not a hard no-execution guarantee. |

English audio lasted about 6.15 seconds. In the silent-prompt case, the first text
arrived about 1.34 seconds after playback began, with 12 updates before playback
ended. A Chinese Tingting sample produced its first text after about 1.54 seconds,
with 28 updates before playback ended. These are single synthetic-speech samples,
not a latency benchmark. Earlier Chinese Eddy samples had omissions and errors;
accuracy was not consistently established. Live reply media packets were received;
speaker playback and natural interruption were not qualified.

Conversational mode returned a final user transcript. Silent-prompt runs did not
return a final transcript even after native stop; they emitted a closed event.
Do not promise automatic final correction from this conversational workaround.
All recorded test thread IDs were absent from the native persistent thread table
after shutdown. No qualification script remained running.

### A separate native desktop streaming dictation implementation exists

Read-only inspection of the installed official desktop application bundle found
the following implementation in `app-initial-4d7ea7f81c2d.js` and
`main-DaMR-wdT.js` inside `Contents/Resources/app.asar`:

- The desktop obtains connection information through
  `/codex/dictation-stream-connect-info`, then connects to `/dictation/stream`.
- `session.start` uses PCM16 audio, `provider_mode: streaming_sse`, and selects
  `transcript_delivery_mode: segment` when an update callback exists, otherwise
  `final_only`. The event schema also recognizes `delta` mode.
- `transcript.segment` and `transcript.final` carry an utterance ID, revision, and
  text. A final revision replaces the provisional text for that utterance.
- `audio.append` streams chunks; `session.close` flushes and completes the session.
- A `codex-app-dictation-streaming` flag and language selection affect whether the
  streaming implementation is used. Code presence does not prove the user's
  active flag values or the exact implementation used by iOS.

This is evidence that progressive dictation is a separate desktop capability;
it does not require Apple ASR merely because iOS displays text progressively.
No conclusion about iOS's private implementation was established.

### Dedicated dictation live-access boundary

Reproduced the desktop's native `getAuthStatus` credential handoff in memory and
used only its observed dictation destination and WebSocket subprotocols. No token
was printed or persisted. Independent handshakes for segment, delta, and final-only
delivery all returned HTTP 403 before session startup; no test audio was sent to
this endpoint. The response does not establish whether the missing prerequisite
is a desktop session, request context, or another server admission condition.
Do not label the user's subscription ineligible or attempt to bypass admission.

Thus subscription streaming recognition is empirically established through V3.
The dedicated dictation protocol has the desired partial/final design, but its
standalone access remains unqualified. It is not yet integrated into Decodex.
