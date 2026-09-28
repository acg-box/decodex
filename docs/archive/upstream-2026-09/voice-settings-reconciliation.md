> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile optional voice settings

Fixed upstream: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Read the complete inherited/current diffs for five voice-setting source files
and both versions of work/voice-settings.md. This batch changes no product code.

| File | Complete disposition |
| --- | --- |
| GPUI chief_voice_settings.rs | Preserve explicit read, selection, one write and readback. The added source invalidation retires the panel epoch when the runtime source or native thread changes. Native-child navigation hides the panel and blocks writes. Choice callbacks require the epoch they displayed. Existing generation, selected-work and task checks still reject late results. |
| GPUI chief_voice_settings_wire_tests.rs | Retain the complete original service-socket fixture: a click followed by Space sends one selection; a lost reply reads fresh effective/preference state without another write or starting audio. Add the source-change epoch regression. |
| Protocol chief_voice_settings.rs | Only clarify the voices field documentation: choices can come from the native catalog or the upstream fallback. No wire field or state is removed. |
| Runtime chief_voice_settings.rs | Add the native connection identity to the review fingerprint and every post-await source comparison. Preserve work/thread/account/generation/revisions/configuration identity, exact thread read, bounded observation, catalog membership, conditional write and uncertain readback. Saved preference and effective project override remain distinct. |
| Runtime chief_voice_settings_tests.rs | Retain the two original source/version/readback tests. Add same-key transport replacement coverage that rejects both the old review and an observation from a retired connection, with zero writes. |
| work/voice-settings.md | Map the original adapter, retained bridge, protocol, runtime and picker sections to the current optional-control document and effective-native-voice.md. Preserve the two original media review sections verbatim under a historical notice; retain their unpassed loopback and signed/audio limits. Earlier protocol numbers and test counts are historical delivery evidence. |

No source function or original test is removed from these five files. The adapter
preference and effective-setting owners are already exact-content entries in the
register. This comparison does not add a second configuration or media owner.

## Preserved media boundary

The inherited mute/backlog review applies native Opus changes to their upstream
owner. Decodex uses WebKit MediaStreamTrack.enabled and browser media queues.
VoiceMediaHost.swift retains that command, fixed capture/runtime/connection error
messages, and cleanup of tracks, peer, channel and audio objects. The retained
startup-failure test injects three stages; it does not prove OS permission.
The explicit WebRTC mute loopback test still requires its opt-in environment and
a usable interactive ICE connection. Its presence is not a passing result.
chief_voice.rs retains failed status across cleanup before the next client poll.

Keep the voice picker optional for the user's subtraction review. The effective
voice read before an existing call is a separate correctness requirement and can
remain without the picker. Removing the picker does not authorize removal of
native source/version checks from other retained configuration writes.

## Validation and limits

Two GPUI service/panel tests and four runtime tests pass on main 030409f62.
The runtime filter covers the three settings cases plus effective voice at call
start; two installed-native opt-in cases remain ignored. Six original hashes
match; all five Rust files retain
every inherited function and test. The restored media sections match the original
bytes. git diff --check passes. This validation covers
the local settings protocol and state owners, not actual microphone capture,
audible voice, WebRTC connectivity or signed desktop interaction. Close only these
six file dispositions. The wider voice-input document and
final media/desktop acceptance remain open. Automations remain paused.
