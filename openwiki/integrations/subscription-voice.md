---
type: Reference
title: "Subscription dictation and live voice"
description: "Native subscription audio ownership, per-call voice settings and on-demand WebRTC media hosting."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-95772fa38252ded838ec3a2f
    resource: repo://.config/nextest.toml
  - id: openwiki-source-759ed0025679548e066b427b
    resource: repo://apps/decodex-gpui/build.rs
  - id: openwiki-source-2e244c19d3ad0a0d54117218
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexTransport/DictationStream.swift
  - id: openwiki-source-7a380101e79db24d163b9af6
    resource: repo://apps/decodex-gpui/src/agent_dictation.rs
  - id: openwiki-source-15d320ea458ddf705d950ba9
    resource: repo://apps/decodex-gpui/src/agent_response_metrics.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-e8d0929bc0eb570f21e8bf45
    resource: repo://apps/decodex-gpui/src/agent_voice_settings.rs
  - id: openwiki-source-32bb36aae256aa57e6e82bd0
    resource: repo://apps/decodex-gpui/src/agent_voice.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-032278b78f4f6cac0e654ef0
    resource: repo://apps/decodex-gpui/src/native_audio_owner_tests.rs
  - id: openwiki-source-684fc3cac9c65ba3343eb51f
    resource: repo://apps/decodex-gpui/src/native_audio_owner.rs
  - id: openwiki-source-bad2d1d628fcf7393964c766
    resource: repo://apps/decodex-gpui/src/native_audio_route.rs
  - id: openwiki-source-edba7179ea148b6abe727c00
    resource: repo://apps/decodex-gpui/src/native_dictation_audio.rs
  - id: openwiki-source-7bc7ef970363495451bd9aac
    resource: repo://apps/decodex-gpui/src/native_dictation_capture.rs
  - id: openwiki-source-95412047c2ed776dd7116977
    resource: repo://apps/decodex-gpui/src/native_live_audio.rs
  - id: openwiki-source-7647aba6b390035d3777c2b5
    resource: repo://apps/decodex-gpui/src/native_voice_audio.rs
  - id: openwiki-source-ca95ba5564216bb4898c8392
    resource: repo://apps/decodex-gpui/src/native_voice_online_tests.rs
  - id: openwiki-source-d1f06f7b22d0c2eec328d95f
    resource: repo://apps/decodex-gpui/src/native_voice_transport.rs
  - id: openwiki-source-5a4dfc4ce84de6c05c56d3fb
    resource: repo://apps/decodex-gpui/src/voice_history_block.rs
  - id: openwiki-source-3127fd4c7b2aefcc9cea7961
    resource: repo://automations/upstream/retained-contracts.json
  - id: openwiki-source-d0b9aaf952250307abc4342e
    resource: repo://crates/decodex-codex/src/app_server_client/archive.rs
  - id: openwiki-source-c990afbb3dfa828de42edbc3
    resource: repo://crates/decodex-codex/src/app_server_client/realtime_preferences.rs
  - id: openwiki-source-e577aa8a2c4a08b3ced5f846
    resource: repo://crates/decodex-codex/src/app_server_client/realtime_settings.rs
  - id: openwiki-source-eccf08613126ff810078b520
    resource: repo://crates/decodex-protocol/src/agent.rs
  - id: openwiki-source-d7ed9b56d246c61c91fd95a4
    resource: repo://crates/decodex-protocol/src/voice.rs
  - id: openwiki-source-a9a90ca34e6ca9f516d3d482
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_voice_settings_start_tests.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-2446905cd66169ccfd107760
    resource: repo://crates/decodex-runtime/src/agent_voice_presentation.rs
  - id: openwiki-source-a11095ee2d127564345b4574
    resource: repo://crates/decodex-runtime/src/agent/conversation_drafts.rs
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
  - id: openwiki-source-042bf1b2b00d2d0ac4dced71
    resource: repo://crates/decodex-webrtc-build/src/archive.rs
  - id: openwiki-source-9d02660394edc50f69d35816
    resource: repo://crates/decodex-webrtc-build/src/lib.rs
  - id: openwiki-source-87ec62f51f8e5d9a99b4bb6e
    resource: repo://crates/decodex-webrtc-build/tests/qualify_trust_roots.py
  - id: openwiki-source-b7b32ffdb5400494a6a36b80
    resource: repo://crates/decodex-webrtc-build/tests/trust_roots.json
  - id: openwiki-source-e1fc2e0e81623c7dd7ff5417
    resource: repo://database/src/agent_process.rs
  - id: openwiki-source-f580db3132c852bedbf0155b
    resource: repo://database/src/agent_voice_presentation.rs
  - id: openwiki-source-bffc76f9844e337b0c8df764
    resource: repo://database/src/agent_voice.rs
  - id: openwiki-source-6197a876b2effbe992b1e2ec
    resource: repo://database/src/agent.rs
  - id: openwiki-source-c9f290c212ee80724ccf643a
    resource: repo://database/src/agent/drafts.rs
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
  - id: openwiki-source-90e2e66c4c17531650b4f54e
    resource: repo://scripts/macos/prepare_voice_acceptance.py
  - id: openwiki-source-2fc96071d03c8f12c8c6fb3a
    resource: repo://scripts/macos/run_native_voice_tests.py
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
generated: { by: "codex", at: "2026-11-18T19:17:42.865Z" }
verified:
  - by: openwiki/0.9.0
    at: 2026-11-18T19:17:42.865Z
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

Dictation and Live share the resource owner in `native_voice_audio.rs` but use separate hardware implementations. Dictation uses `AVAudioEngine` with Apple's Voice Processing enabled, bypass disabled and automatic gain control enabled. `native_dictation_audio.rs` sends the processed Float32 input to an `AVAudioSinkNode` at 24 kHz mono, then the existing capture worker converts it to PCM16 for upload. Apple's I/O unit owns noise suppression, gain control and sample-rate conversion. Configuration fails if Voice Processing or automatic gain cannot be enabled. The required duplex output renders silence with the minimum other-audio ducking setting. Captured speech does not play through the speakers. Live uses WebRTC's platform audio device and software echo cancellation, noise suppression and gain control. Swift retains permission and device discovery.

The waveform is a display meter, not a speech detector. Dictation supplies levels from Apple's processed input; Live supplies levels from its native media path. Neither applies an extra display gain before the shared meter. It maps -42 to -12 dBFS to the visible range and keeps lower levels flat. The shared UI motion smooths the bars. This display floor does not remove, gate or change captured PCM. Dictation does not add a separate software noise gate.

The shared `native_audio_owner.rs` thread caches a stopped resource for each mode but leases only one session at a time. Preparation does not start recording. Dictation pauses the engine to stop audio hardware while retaining prepared resources for the next start. It returns its PCM queues for reuse; a released Live factory is replaced and the next one is prepared. Start resets the dictation nodes and clears old capture and playback buffers before enabling I/O. Finish stops hardware and drains the remaining dictation samples. Cancellation stops hardware independently of network teardown, and identity checks discard late releases from an old session. The owner rebuilds dictation after input, output or engine configuration changes while retaining session queues. Explicit input selection occurs before Voice Processing is enabled; the system-default input keeps Apple's aggregate route. The first received dictation PCM sample emits the capture-ready event and the `dictation_sink_first_pcm_ms` diagnostic. This measures capture startup, not subscription connection time or time to recognized text. Preparation and buffering do not establish a latency guarantee. First-word completeness, microphone quality, device switching and playback effects require installed acceptance.

The versioned `decodex_dictation_create_v2` symbol binds the Rust session to the matching native message contract. This adapter remains in the existing signed application library.

## Live voice

The coordinator uses native `thread/realtime/start` and `thread/realtime/stop` with the owned Agent thread and connection generation. A prior text turn is not required. Starting from a local editor reserves its conversation scope when signaling begins. The editor captures its model, reasoning, speed and permission settings for native thread creation. The host prepares the connection, and the coordinator uses the existing fenced thread-creation path. Realtime then starts without an ordinary text turn. Native transcript deltas and completion events update durable conversation observations; they are not simulated worker steps.

The desktop starts local media preparation immediately instead of requiring a voice-settings read from a thread that does not yet exist. The service checks managed voice policy before native thread creation and reads the effective voice before realtime signaling. Existing threads retain their native context. Connection and media errors use the existing conversation feedback.

Local transcript storage keeps newer received text when a delayed final is a shorter prefix of that text. The saved text remains marked incomplete. An empty final does not erase the received tail; normal closure can save it. Expanded finals and normal transcription corrections still replace provisional text. Saving a transcript never resubmits it as native input.

An active call keeps its local authorization until native stop or positive process death. Empty and whitespace-only transcript text is not saved. Closure saves nonempty received tails, then removes the local call record if it has no stored transcript. This also applies to an empty failed call. The call does not remain in local voice history after a database reopen. Already-started task ownership remains intact. A transport disconnect alone does not prove closure and keeps its existing recovery behavior.

An unused Live reservation does not appear as a saved conversation. Received transcript text or real native work publishes it. The transient native thread-creation dispatch fence does not publish an empty reservation. Cleanup still requires idle dispatch and no active call, so an unpublished reservation cannot be removed while creation is unresolved. Desktop refresh retains a local unpublished call owner; confirmed removal of a previously published owner and explicit archive still stop capture. After closure or startup recovery, cleanup checks local content and native history before removing an empty reservation through native deletion and the existing durable deletion receipt. A fresh native thread can be absent from both archive catalogs before its first turn; cleanup does not require an archive entry. After a host restart, cleanup resumes the exact saved thread and attaches it to the current admitted process before deletion. A pending deletion receipt does not publish an empty conversation. If that receipt exists and an exact read plus both complete native catalogs confirm absence, recovery completes local cleanup without resuming or deleting the native thread again. A native turn preserves the conversation and records the observed content without submitting it again. The built-in Assistant identity and existing project Agents are retained. Internal reservation and publication receipts are excluded from transcript display.

The UI also discards blank retained captions when local media ends. A complete nonempty call has one expandable record that reuses the existing steps divider, with a topic overview and the shared response footer. See [voice history and pagination](../architecture/conversation-presentation.md#voice-history-and-pagination).

The Rust media owner creates a native libwebrtc peer connection for each live call. No WebView or JavaScript media runtime is used. Swift handles microphone permission and device discovery; Rust owns WebRTC's platform audio-device lifecycle. The service retains subscription authentication, session binding and native RPC.

The Live factory selects the requested input and default output without starting I/O during preparation. Recording and playback start only after the peer connection and ordered caption channel are ready. The production call uses WebRTC's platform audio device. A separate test PCM path uses 48 kHz mono frames and a 10 ms transport loop; that path is not the production microphone implementation. Mute disables the outgoing track, including when requested before negotiation.

Closing the Rust media owner cancels its capture and peer connection worker. The shared owner stops hardware independently of peer teardown and prepares a new Live factory after release. Provider findings can retire local microphone authority even if stop acknowledgment is lost. Unknown native stop outcomes must not be treated as a guaranteed remote stop.

The native media ABI is version 4. The static libwebrtc archive needs the Objective-C linker flag in the GPUI build script. Bundles include its license notices under `Resources/ThirdPartyNotices`. Subscription voice does not switch to API-key billing.

Default macOS acquisition uses the Decodex archive guard with the selected official
WebRTC release. The guard checks the complete ZIP digest and strict header layout
before it extracts or repairs the cache. The selected release and archive digests
are owned by `crates/decodex-webrtc-build/src/lib.rs`. Historical artifact and
transport qualifications retain their original scope in the
[contract catalog](../../automations/upstream/retained-contracts.json).

The retained trust-anchor probe checks the distributed header and certificate DER
bytes in the static library. It verifies that the retired AAA root is absent and
that a known control root remains. This artifact check complements the real WebRTC
transport tests; it does not execute a TURN/TLS handshake or qualify a live
provider. See the [archive procedure](../operations/commands-and-validation.md#qualify-native-dependency-artifacts).


## Call previews

A folded Live record shows a preview of its original transcript. The desktop uses the first nonempty user utterance, or the first available speech when no user utterance exists. It converts Markdown to plain text, normalizes whitespace and retains at most 160 characters with an ellipsis for shortened text. The spoken wording and language stay intact. A call containing only **Mm-hmm.** previews **Mm-hmm.**.

The preview is available directly from the loaded transcript. Opening the record replaces the preview with the complete loaded speaker transcript. Call history reads attach only recorded timing and estimated spend; they do not schedule model inference, generate an overview or persist new summary metadata. Earlier overview fields are ignored by the current receipt projection, and legacy overview events remain excluded from transcript messages.

Live uses the same Time, Input, Output and Spend footer as ordinary replies in either disclosure state, with shared details and transcript copy. Time is the local authorization-to-closure interval. Spend estimates this interval at the [published desktop Voice rate](https://learn.chatgpt.com/docs/pricing#how-much-does-voice-cost) of $0.05 per minute, checked November 14, 2026. It uses integer USD millionths and displays the shared estimate mark. Backend model work has separate token-based pricing. The detail text identifies this scope and elapsed-time basis. The interval is an estimate rather than a provider-reported billed duration.

The current call projection has no independent token receipts, so Input and Output stay unknown and the common details explain why. Task-turn counts, thread totals and transcript character counts do not fill those fields. Database checks cover timing after reopen and retained late speech. The rendered four-turn fixture checks all four fields in both disclosure states, folded copy, complete expanded speaker turns, the 6-point divider content gap and the standard response footer gap. These checks do not establish physical microphone quality or installed-window visual acceptance.

## Call ownership while navigating

The accepted [Assistant lifecycle design](../decisions/assistant-lifecycle.md) preserves this exact call owner. Automatic native-context renewal waits for a suitable call boundary; it must not change the call's native thread or stop capture merely to refresh text context.

Browsing another conversation, the Work overview or a new conversation keeps the Live call attached to its original work and session. A background bar identifies that conversation and provides Return, Mute and End controls. Other conversations retain their own text editors and drafts; their text does not become input to the call.

Ending the call retires its media owner. Confirmed removal or archive of the call's owner also stops local capture. Navigation alone does not stop it. The tests for this behavior use synthetic UI and media fixtures; they do not requalify background OS audio, physical devices or a live provider.

## UI and verification

The normal composer receives dictation text. The conversation that owns a Live call shows its waveform in that area. The selected input device applies to capture, not to a model-selector control.

Focused tests live in the Rust dictation/voice modules and Swift `VoiceMediaHostTests`. Rust capture tests cover PCM conversion and terminal delivery, while composer tests cover early audio buffering, continued capture feedback during a pending network request, and draining audio before Finish. Rust `dictation_transcript.rs` tests cover segment order, stale revisions, final corrections and transcript limits. The native transport loopback test covers offer/answer, bidirectional audio, captions, mute before negotiation, subsequent mute changes and buffer release. Opt-in tests in `native_audio_owner_tests.rs` exercise hardware capture, playback, mode changes, preparation and route changes. Dictation hardware checks read back the 24 kHz mono format, enabled and non-bypassed Voice Processing, and enabled automatic gain. They also check repeated startup, valid PCM and no further samples after the final buffer is drained. These test definitions do not establish speech-recognition accuracy, audible quality, echo cancellation quality or provider entitlement. Live acceptance separately requires microphone permission, selected-device capture, partial text, final correction, and both live transcript roles. Unit tests do not establish provider entitlement or network latency.

### Run native WebRTC tests with a development signature

On macOS, select an existing development certificate in the local Keychain. Set `DECODEX_APP_SIGN_IDENTITY` to its name or fingerprint, then run:

```sh
DECODEX_APP_SIGN_IDENTITY="$EXISTING_DEVELOPMENT_IDENTITY" cargo +stable make test-native-voice
```

The task builds the locked GPUI test target with all features. It signs a separate copy at `target/native-voice-tests/decodex-native-voice-tests` with the fixed identifier `box.acg.decodex.tests.native-voice`, then uses Nextest build metadata to run the four retained transport tests. Cargo's original executable stays unchanged. The task serializes the signed copy, verifies its signature, and fails if a selected test is missing or ignored. A missing identity or ad hoc `-` identity is rejected before the build. General workspace test commands keep their existing behavior.

First execution can require an explicit system firewall allow. In the bounded signing qualification, the first pre-consent run failed while waiting for `connected`. After the user allowed it, the original and a genuinely rebuilt executable each passed all four tests. Their code hashes differed and their designated signing requirements matched. Each test window contained 28 known allow events, with no queued or prompt events. Two additional task runs passed all four tests, for 16 post-consent executions. The initial failure remains a separate result in the [retained qualification](../../automations/upstream/retained-contracts.json).

This qualifies the measured macOS arm64 host, development certificate and stable path. Certificate renewal, a different team, identifier or path, OS or firewall policy changes, and a locked session remain unmeasured. The tests retain real WebRTC packet flow and synthetic PCM; they do not open the microphone or speaker. No automatic firewall rule edit, installation or release occurs. The historical audio-energy failure after connection still has no confirmed cause; this result does not attribute that later assertion to the firewall.

### Diagnose native media ICE timeouts

`native_call_exchanges_audio_captions_mutes_and_releases_buffers` and `remote_channel_loss_reports_failure_and_releases_buffers` share the same native ICE connection fixture. Both use synthetic PCM, with device capture and playback disabled. Nextest reserves the runner for each media exchange. A timeout waiting for `connected` occurs before the audio, caption, mute and release assertions.

Run the exact workspace feature combination:

```sh
cargo +stable nextest run --locked --workspace --all-targets --all-features \
  -E 'binary_id(decodex-gpui::bin/decodex-gpui) and (test(native_call_exchanges_audio_captions_mutes_and_releases_buffers) or test(remote_channel_loss_reports_failure_and_releases_buffers))' \
  --stress-count 20 --retries 0 --no-fail-fast
```

The observed `cargo-nextest` 0.9.143 can return exit code zero after an earlier stress iteration fails when `--no-fail-fast` is used. Inspect every iteration and the final summary; do not use the exit code alone to mark the run as passed. The [nextest 0.9.148 changelog](https://nexte.st/changelog/#0.9.148) records the upstream fix. The [retained evidence](../../automations/upstream/retained-contracts.json) keeps the original failed iteration separately from later passes.

These tests need incoming network access for the actual compiled test executable. A package-only build and a workspace feature build can have different executable identities and firewall decisions. On macOS, inspect the application firewall decision for that executable and its `socketfilterfw` events at the failure time. A saved allow rule alone does not show that a particular flow received an allow verdict. [Apple documents](https://support.apple.com/guide/mac-help/block-connections-to-your-mac-with-a-firewall-mh34041/mac) that connections remain denied while an access alert awaits a response. Do not disable the firewall, add retries, remove candidates or extend deadlines to conceal this prerequisite.

The `retained_project_agent_work.native_media_followup` section in [retained evidence](../../automations/upstream/retained-contracts.json) preserves the original two failed results and the separate passing reruns. The audio sources, dependencies and WebRTC features match across the pre-change baseline and current main. Each source passed 40 repeated media executions without retries; current main also passed all 2918 workspace tests. Historical firewall events support an external admission cause. Recovered `loginwindow` info/debug events place both failed workspace tests within the locked interval. The already-allowed package-only baseline also passed both tests while locked. After unlock, the workspace test binary received a firewall prompt, then a known allow verdict. This supports an admission request left unattended during locking; it does not show that locking alone broke audio. The evidence is retrospective, with no new controlled lock or firewall-denial experiment. Physical audio, provider behavior and installed-app acceptance remain separate.

For a lock correlation, include the preceding lock and subsequent unlock in the log time window. Read the info/debug state transitions as well as the firewall verdicts. A short window around the timeout can omit the state transition and cannot establish that the session stayed unlocked.

### Authorized local hardware checks

The later `media_hardware_followup` record in the [retained evidence](../../automations/upstream/retained-contracts.json) qualifies one host's current physical USB input. With explicit microphone permission, three existing opt-in tests passed: deferred Live capture, repeated dictation/Live capture with release before transport teardown, and native device PCM delivery to a local peer across dictation. The two synthetic ICE/media tests also passed separately. No retries, saved microphone audio, uploads or system route changes were used.

Resolve the input ID locally and set `DECODEX_AUDIO_TEST_DEVICE_ID` before running:

```sh
cargo +stable nextest run --locked --workspace --all-targets --all-features \
  --run-ignored only --test-threads 1 --retries 0 --no-fail-fast \
  -E 'binary_id(decodex-gpui::bin/decodex-gpui) and (test(prepared_live_reserves_its_factory_without_starting_io) or test(production_owner_captures_across_modes_and_stops_before_transport_teardown) or test(native_device_calls_use_prewarmed_dsp_across_dictation))'
```

The current output was a virtual route. These checks establish capture, local PCM flow and device lifecycle; they do not establish physical-speaker output, speech intelligibility or audible quality. The full workspace suite was not repeated. No controlled lock experiment, provider voice session, installed-app test, installation or release occurred. The complete temporary acceptance directory and its exact-owned helper processes were removed after private database and source receipts were preserved.

See [Agent coordination](../architecture/chief-coordination.md) and [Desktop workspace](../architecture/desktop-workspace.md).

### Authorized synthetic online checks

The later `online_voice_acceptance` entry in the [retained index](../../automations/upstream/retained-contracts.json) qualifies one existing subscription connection with local TTS input. The final run uses fresh product and native homes and the same verified CLI, Code Mode helper and search helper. The production voice implementation did not change during that earlier qualification.

| Check | Observed result |
| --- | --- |
| Subscription dictation | The synthetic words were transcribed to final text. Agent history stayed unchanged. |
| Native V3 WebRTC voice | Final assistant-role text and nonzero decoded speech returned. Explicit Stop reached Ended, with no active call, Agent turn or new Work left. |
| Separate local playback | The user heard an actual reply and reported low volume. The test player used 0.5 gain and then 1.0; system volume and routing stayed unchanged. |

The two explicit online tests passed with 1023 skipped, no unmatched selector, retry or LEAK. Strict lint for 13 packages and formatting passed. The [reusable input generator](../../scripts/macos/prepare_voice_acceptance.py) creates mono PCM16 at 24 kHz and 48 kHz. The ignored tests use the production service and manual PCM media branch with device recording and playout disabled. They never open a microphone. See [commands and prerequisites](../operations/commands-and-validation.md#synthetic-online-voice-acceptance).

This is a bounded synthetic sample. Separate playback through the current virtual bridge does not qualify the installed UI's microphone-to-device-ADM path, all downstream devices, broad loudness, intelligibility or echo cancellation. No microphone audio was saved or uploaded. Temporary homes, credentials, components and audio were removed after private domain/history evidence was saved. One initial synthetic trial in the normal native home is confirmed archived; its inactive history remains native-owned. Official Codex Project presentation remains unobserved. No installation or release was needed for this follow-up.

## Retained voice preferences and limitations

The voice preference picker uses the native V3 catalog. Missing, empty or malformed V3 data is an error. The catalog supplies the choices for an explicit voice preference. When the effective voice is unset, the picker shows **Server default** and the V3 call destination selects the voice.

The picker reads effective project settings, then writes one reviewed `realtime.voice` preference with native version checking. It affects the next call. Project settings can override the saved preference. At each call start, the coordinator reads the effective voice for the exact native thread. It passes a supported voice to V3 or sends a null voice to use the destination default. The request retains startup context, requests transcript-tail flush at session end, and enables `backendReasoningStatus`. Codex can relay completed public reasoning summaries as quiet V3 context. Optional model and start and end instructions stay bound to that call.

Configuration and synthetic media tests do not establish real microphone, WebRTC, audible next-call selection or late remote caption identity. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md). Memory configuration is separate from audio capture; see [Models and settings](../workflows/models-and-settings.md).
