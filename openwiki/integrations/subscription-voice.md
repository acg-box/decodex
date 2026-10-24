---
type: Reference
title: "Subscription dictation and live voice"
description: "Native subscription audio ownership, per-call voice settings and on-demand WebRTC media hosting."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-759ed0025679548e066b427b
    resource: repo://apps/decodex-gpui/build.rs
  - id: openwiki-source-2e244c19d3ad0a0d54117218
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexTransport/DictationStream.swift
  - id: openwiki-source-7a380101e79db24d163b9af6
    resource: repo://apps/decodex-gpui/src/agent_dictation.rs
  - id: openwiki-source-32bb36aae256aa57e6e82bd0
    resource: repo://apps/decodex-gpui/src/agent_voice.rs
  - id: openwiki-source-032278b78f4f6cac0e654ef0
    resource: repo://apps/decodex-gpui/src/native_audio_owner_tests.rs
  - id: openwiki-source-684fc3cac9c65ba3343eb51f
    resource: repo://apps/decodex-gpui/src/native_audio_owner.rs
  - id: openwiki-source-7bc7ef970363495451bd9aac
    resource: repo://apps/decodex-gpui/src/native_dictation_capture.rs
  - id: openwiki-source-09e15400c0eb6fa34f087b07
    resource: repo://apps/decodex-gpui/src/native_input_queue.rs
  - id: openwiki-source-95412047c2ed776dd7116977
    resource: repo://apps/decodex-gpui/src/native_live_audio.rs
  - id: openwiki-source-d1f06f7b22d0c2eec328d95f
    resource: repo://apps/decodex-gpui/src/native_voice_transport.rs
  - id: openwiki-source-c990afbb3dfa828de42edbc3
    resource: repo://crates/decodex-codex/src/app_server_client/realtime_preferences.rs
  - id: openwiki-source-91dfc90839432583cfb2a467
    resource: repo://crates/decodex-runtime/src/agent/voice_persistence_tests.rs
  - id: openwiki-source-b415683824e98dc4ed730738
    resource: repo://crates/decodex-runtime/src/agent/voice.rs
  - id: openwiki-source-2465dd41e7771bea4f7b9c61
    resource: repo://crates/decodex-runtime/src/dictation_native.rs
  - id: openwiki-source-58b9baa7130e82ec8b830513
    resource: repo://crates/decodex-runtime/src/dictation_transcript.rs
  - id: openwiki-source-7b941e7c2c91cb7415f05243
    resource: repo://crates/decodex-runtime/src/dictation.rs
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
generated: { by: "codex", at: "2026-10-21T09:01:22.045Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-24T21:11:31.338Z
---

# Subscription dictation and live voice

Two flows share microphone presentation but have different semantics.

| Flow | Input and output | Execution |
| --- | --- | --- |
| Dictation | Audio becomes an editable composer draft, with segment updates and final corrections | Does not send a text turn by itself |
| Live voice | Native real-time session exchanges speech; transcripts become conversation history | Uses the existing Agent native thread |

## Dictation

`DictationGateway` owns one ephemeral session. It obtains subscription authentication from the retained native connection through `getAuthStatus`. The token stays in the service and signed native URLSession adapter. It is not a second API-key configuration or a system speech-recognition fallback.

The Swift URLSession adapter connects to the subscription dictation stream and transports bounded messages. Rust constructs the session configuration and audio messages, validates PCM16, 24 kHz, mono input, and interprets server events. The Rust session also owns segment order, revision numbers and final markers. Final and partial revisions are separate streams. A final supersedes provisional text; only a current final can revise finalized text. The Swift adapter forwards raw server messages; if its event queue fills, it reports a terminal error instead of discarding accepted segments. The final transcript corrects the same composer draft rather than opening a second editor.

Audio, authentication and the draft are not stored in SQLite by this gateway. Starting without a ready account fails clearly. The composer buffers captured audio while the subscription handshake is pending and drains that audio before sending Finish. The service rejects audio submitted before its Listening state; the UI therefore waits for that state before upload. Capture feedback continues while a network request is pending. Disconnect before final correction preserves received text and does not replay audio. The endpoint is a subscription transport dependency, not a promise that every account or future server version supports it.

Dictation and Live share the resource owner in `native_voice_audio.rs` but use separate hardware implementations. Dictation uses an input-only Apple Audio Queue, with mono Float32 capture at the requested 24 kHz rate and conversion to PCM16 for upload. It has no playback graph. Live uses WebRTC's platform audio device and software echo cancellation, noise suppression and gain control. Swift retains permission and device discovery.

The shared `native_audio_owner.rs` thread caches a stopped resource for each mode but leases only one session at a time. Preparation does not start recording. Dictation returns its PCM queues for reuse; a released Live factory is replaced and the next one is prepared. Start clears old capture and playback buffers before enabling I/O. Finish stops hardware and drains the remaining dictation samples. Cancellation stops hardware independently of network teardown, and identity checks discard late releases from an old session. The owner handles hardware route changes while retaining session queues. The first received dictation PCM sample emits the Listening event and the `dictation_sink_first_pcm_ms` diagnostic. This measures capture startup, not subscription connection time or time to recognized text. Preparation and buffering do not establish a latency guarantee. First-word completeness, microphone quality, device switching and playback effects require installed acceptance.

The versioned `decodex_dictation_create_v2` symbol binds the Rust session to the matching native message contract. This adapter remains in the existing signed application library.

## Live voice

The coordinator uses native `thread/realtime/start` and `thread/realtime/stop` with the existing owned Agent thread and connection generation. Start requires a ready manager work item. Native transcript deltas and completion events update durable conversation observations; they are not simulated worker steps.

Local transcript storage keeps newer received text when a delayed final is a shorter prefix of that text. The saved text remains marked incomplete. An empty final does not erase the received tail; normal closure can save it. Expanded finals and normal transcription corrections still replace provisional text. Saving a transcript never resubmits it as native input.

The Rust media owner creates a native libwebrtc peer connection for each live call. No WebView or JavaScript media runtime is used. Swift handles microphone permission and device discovery; Rust owns WebRTC's platform audio-device lifecycle. The service retains subscription authentication, session binding and native RPC.

The Live factory selects the requested input and default output without starting I/O during preparation. Recording and playback start only after the peer connection and ordered caption channel are ready. The production call uses WebRTC's platform audio device. A separate test PCM path uses 48 kHz mono frames and a 10 ms transport loop; that path is not the production microphone implementation. Mute disables the outgoing track, including when requested before negotiation.

Closing the Rust media owner cancels its capture and peer connection worker. The shared owner stops hardware independently of peer teardown and prepares a new Live factory after release. Provider findings can retire local microphone authority even if stop acknowledgment is lost. Unknown native stop outcomes must not be treated as a guaranteed remote stop.

The native media ABI is version 4. The static libwebrtc archive needs the Objective-C linker flag in the GPUI build script. Bundles include its license notices under `Resources/ThirdPartyNotices`. Subscription voice does not switch to API-key billing.

## UI and verification

The normal composer receives dictation text. Live voice uses its own waveform/voice state in that area. The selected input device applies to capture, not to a model-selector control.

Focused tests live in the Rust dictation/voice modules and Swift `VoiceMediaHostTests`. Rust capture tests cover PCM conversion and terminal delivery, while composer tests cover early audio buffering, continued capture feedback during a pending network request, and draining audio before Finish. Rust `dictation_transcript.rs` tests cover segment order, stale revisions, final corrections and transcript limits. The native transport loopback test covers offer/answer, bidirectional audio, captions, mute before negotiation, subsequent mute changes and buffer release. Opt-in tests in `native_audio_owner_tests.rs` exercise hardware capture, playback, mode changes, preparation and route changes. These test definitions do not establish audible quality, echo cancellation quality or provider entitlement. Live acceptance separately requires microphone permission, selected-device capture, partial text, final correction, and both live transcript roles. Unit tests do not establish provider entitlement or network latency.

See [Agent coordination](../architecture/chief-coordination.md) and [Desktop workspace](../architecture/desktop-workspace.md).

## Retained voice preferences and limitations

The voice preference picker uses the nonempty native V3 catalog. It falls back to V1 only when V3 is missing, null or empty; a malformed V3 value is an error. The native `defaultV1` field supplies the shared default and must belong to the selected catalog. There is no local fixed voice whitelist.

The picker reads effective project settings, then writes one reviewed `realtime.voice` preference with native version checking. It affects the next call and does not restart active audio. Project settings can override the saved preference. At each call start, the coordinator reads the effective voice for the exact native thread and passes it to the v3 real-time request. The request retains startup context and requests transcript-tail flush at session end. Optional model and start/end instructions stay bound to that call.

Configuration and synthetic media tests do not establish real microphone, WebRTC, audible next-call selection or late remote caption identity. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md). Memory configuration is separate from audio capture; see [Models and settings](../workflows/models-and-settings.md).
