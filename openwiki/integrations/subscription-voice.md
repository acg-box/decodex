---
type: Reference
title: "Subscription dictation and live voice"
description: "Native subscription audio ownership, per-call voice settings and on-demand WebRTC media hosting."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-759ed0025679548e066b427b
    resource: repo://apps/decodex-gpui/build.rs
  - id: openwiki-source-3e2768126a812e862f175d21
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/DictationCapture.swift
  - id: openwiki-source-2e244c19d3ad0a0d54117218
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexTransport/DictationStream.swift
  - id: openwiki-source-32bb36aae256aa57e6e82bd0
    resource: repo://apps/decodex-gpui/src/agent_voice.rs
  - id: openwiki-source-7647aba6b390035d3777c2b5
    resource: repo://apps/decodex-gpui/src/native_voice_audio.rs
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
generated: { by: "codex", at: "2026-09-30T09:06:10.197Z" }
verified:
  - by: openwiki/0.6.1
    at: 2026-09-30T14:27:56.062Z
---

# Subscription dictation and live voice

Two flows share microphone presentation but have different semantics.

| Flow | Input and output | Execution |
| --- | --- | --- |
| Dictation | Audio becomes an editable composer draft, with segment updates and final corrections | Does not send a text turn by itself |
| Live voice | Native real-time session exchanges speech; transcripts become conversation history | Uses the existing Agent native thread |

## Dictation

`DictationGateway` owns one ephemeral session. It obtains subscription authentication from the retained native connection through `getAuthStatus`. The token stays in the service and signed native URLSession adapter. It is not a second API-key configuration or a system speech-recognition fallback.

The Swift URLSession adapter connects to the subscription dictation stream and transports bounded messages. Rust constructs the session configuration and audio messages, validates PCM16, 24 kHz, mono input, and interprets server events. The Rust session also owns segment order, revision numbers and final markers. An older revision cannot replace newer text, and finalized segments remain stable. The Swift adapter forwards raw server messages; if its event queue fills, it reports a terminal error instead of discarding accepted segments. The final transcript corrects the same composer draft rather than opening a second editor.

Audio, authentication and the draft are not stored in SQLite by this gateway. Starting without a ready account fails clearly. Audio arriving before readiness is rejected. Disconnect before final correction preserves received text and does not replay audio. The endpoint is a subscription transport dependency, not a promise that every account or future server version supports it.

Native dictation capture uses AVAudioEngine voice processing. The capture enables it while the engine is stopped, selects the requested input device, then reads the processed format for PCM conversion. Other-audio ducking uses the minimum level. Actual microphone quality, device switching and playback effects require installed acceptance; the conversion test does not measure them.

The versioned `decodex_dictation_create_v2` symbol binds the Rust session to the matching native message contract. This adapter remains in the existing signed application library.

## Live voice

The coordinator uses native `thread/realtime/start` and `thread/realtime/stop` with the existing owned Agent thread and connection generation. Start requires a ready manager work item. Native transcript deltas and completion events update durable conversation observations; they are not simulated worker steps.

Local transcript storage keeps newer received text when a delayed final is a shorter prefix of that text. The saved text remains marked incomplete. An empty final does not erase the received tail; normal closure can save it. Expanded finals and normal transcription corrections still replace provisional text. Saving a transcript never resubmits it as native input.

The Rust media owner creates a native libwebrtc peer connection for each live call. No WebView or JavaScript media runtime is used. Swift handles microphone permission and device discovery; Rust calls Apple AVAudioEngine through Objective-C bindings for live capture and playback. The service retains subscription authentication, session binding and native RPC.

One AVAudioEngine processes both microphone and remote playback audio. All client connections use 48 kHz mono; Apple handles hardware conversion and voice processing. Bounded PCM queues connect the audio callbacks to a 10 ms transport loop. The libwebrtc device module and duplicate audio processing are disabled. The ordered data channel carries caption events, and readiness requires both the peer connection and that channel. Mute disables the outgoing track, including when requested before negotiation.

Closing the Rust media owner stops the Apple engine and cancels the peer connection worker. Each call has separate queues. Provider findings can retire local microphone authority even if stop acknowledgment is lost. Unknown native stop outcomes must not be treated as a guaranteed remote stop.

The native media ABI is version 3. The static libwebrtc archive needs the Objective-C linker flag in the GPUI build script. Bundles include its license notices under `Resources/ThirdPartyNotices`. Subscription voice does not switch to API-key billing.

## UI and verification

The normal composer receives dictation text. Live voice uses its own waveform/voice state in that area. The selected input device applies to capture, not to a model-selector control.

Focused tests live in the Rust dictation/voice modules and Swift `DictationCaptureTests` and `VoiceMediaHostTests`. Rust `dictation_transcript.rs` tests cover segment order, stale revisions, final corrections and transcript limits. The native transport loopback test covers offer/answer, bidirectional audio, captions, mute before negotiation, subsequent mute changes and buffer release. The opt-in Apple hardware test checks capture and playback on one engine. Neither test proves audible quality, echo cancellation quality, device changes or provider entitlement. Live acceptance separately requires microphone permission, selected-device capture, partial text, final correction, and both live transcript roles. Unit tests do not establish provider entitlement or network latency.

See [Agent coordination](../architecture/chief-coordination.md) and [Desktop workspace](../architecture/desktop-workspace.md).

## Retained voice preferences and limitations

The voice preference picker reads native catalog and effective project settings, then writes one reviewed `realtime.voice` preference with native version checking. It affects the next call and does not restart active audio. Project settings can override the saved preference. At each call start, the coordinator reads the effective voice for the exact native thread and passes it to the v3 real-time request. The request retains startup context and requests transcript-tail flush at session end. Optional model and start/end instructions stay bound to that call.

Configuration and synthetic media tests do not establish real microphone, WebRTC, audible next-call selection or late remote caption identity. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md). Memory configuration is separate from audio capture; see [Models and settings](../workflows/models-and-settings.md).
