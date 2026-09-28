---
type: Reference
title: "Subscription dictation and live voice"
description: "Subscription dictation and live voice"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T02:19:36.307Z
sources:
  - id: openwiki-source-2e244c19d3ad0a0d54117218
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexTransport/DictationStream.swift
  - id: openwiki-source-c990afbb3dfa828de42edbc3
    resource: repo://crates/decodex-codex/src/app_server_client/realtime_preferences.rs
  - id: openwiki-source-b415683824e98dc4ed730738
    resource: repo://crates/decodex-runtime/src/agent/voice.rs
  - id: openwiki-source-2465dd41e7771bea4f7b9c61
    resource: repo://crates/decodex-runtime/src/dictation_native.rs
  - id: openwiki-source-7b941e7c2c91cb7415f05243
    resource: repo://crates/decodex-runtime/src/dictation.rs
generated: { by: "codex", at: "2026-09-28T02:19:36.307Z" }
---


# Subscription dictation and live voice

Two flows share microphone presentation but have different semantics.

| Flow | Input and output | Execution |
| --- | --- | --- |
| Dictation | Audio becomes an editable composer draft, with segment updates and final corrections | Does not send a text turn by itself |
| Live voice | Native real-time session exchanges speech; transcripts become conversation history | Uses the existing Agent native thread |

## Dictation

`DictationGateway` owns one ephemeral session. It obtains subscription authentication from the retained native connection through `getAuthStatus`. The token stays in the service and signed native URLSession adapter. It is not a second API-key configuration or a system speech-recognition fallback.

The Swift transport connects to the subscription dictation stream and sends PCM16, 24 kHz, mono audio. It handles segment revision numbers and final markers; an older revision cannot replace newer text, and finalized segments remain stable. The final transcript corrects the same composer draft rather than opening a second editor.

Audio, authentication and the draft are not stored in SQLite by this gateway. Starting without a ready account fails clearly. Audio arriving before readiness is rejected. Disconnect before final correction preserves received text and does not replay audio. The endpoint is a subscription transport dependency, not a promise that every account or future server version supports it.

## Live voice

The coordinator uses native `thread/realtime/start` and `thread/realtime/stop` with the existing owned Agent thread and connection generation. Start requires a ready manager work item. Native transcript deltas and completion events update durable conversation observations; they are not simulated worker steps.

The Swift media host owns capture/playback and device selection. The service owns session binding and native RPC. Provider findings can retire local microphone authority even if stop acknowledgment is lost. Unknown native stop outcomes must not be treated as a guaranteed remote stop.

## UI and verification

The normal composer receives dictation text. Live voice uses its own waveform/voice state in that area. The selected input device applies to capture, not to a model-selector control.

Focused tests live in the Rust dictation/voice modules and Swift `DictationCaptureTests`, `DictationTranscriptTests`, and `VoiceMediaHostTests`. Live acceptance separately requires microphone permission, selected-device capture, partial text, final correction, and both live transcript roles. Unit tests do not establish provider entitlement or network latency.

See [Agent coordination](../architecture/chief-coordination.md) and [Desktop workspace](../architecture/desktop-workspace.md).

## Retained voice preferences and limitations

O22 and O25 remain retained product capabilities. The voice preference picker reads native catalog and effective project settings, then writes one reviewed `realtime.voice` preference with native version checking. It affects the next call and does not restart active audio. Project settings can override the saved preference. The effective-voice read in call startup remains a separate correctness responsibility.

Configuration and synthetic media tests do not establish real microphone, WebRTC, audible next-call selection or late remote caption identity. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md) and the [dated voice records](../../docs/archive/upstream-2026-09/voice-settings.md). No external memory service is part of this integration; the retained [memory research](../../docs/archive/upstream-2026-09/agent-memory-research.md) records a native-first decision rather than a new local backend.
