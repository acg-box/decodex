> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Optional native voice preferences

Classification: optional product control for the fixed manual catch-up.
Upstream: `9c9451131fefe6c76e6dd97e8098300298442e64`, inspected at
`595cc91e8cbb1c2ca822d0311dcf12709410c582`.
The existing call now reads effective settings before start; see
[effective native voice](effective-native-voice.md).

## Native and local authority

Read the native catalog and effective project configuration through the retained
process. Keep the writable user layer, its path and expected version separate
from the effective voice. An explicit choice uses one conditional config/batchWrite
for realtime.voice with reloadUserConfig=false, then reads effective settings.
This operation neither restarts live audio nor changes the active call's voice.
Native catalog failure uses the upstream built-in V1/V3 catalog. Unsupported reads
make the settings view unavailable; they do not authorize an unreviewed save.

The adapter binds the reviewed object to its native connection. Protocol 2.82 introduced
a task-scoped settings query and explicit SetVoicePreference action. Runtime
review identity binds task, thread, account revision, process generation, history
revision, transport connection and configuration fingerprint. It rechecks the
current source before and after awaited work. A lost reply or failed readback is
unconfirmed and never causes an automatic write retry.

## Desktop

The settings area offers Voice settings, Refresh voices and explicit voice choices.
It distinguishes the saved preference from the project's effective selection.
Saving affects the next conversation. The panel is hidden while viewing a native
child agent. Task/service reset or changed native source retires the panel epoch;
late responses and old choice callbacks cannot replace or act on the new panel.
The existing explicit command receipt and same-UID client transport remain owners.

## Evidence and remaining acceptance

Installed Codex0.155.0-alpha.16.4 tests use isolated homes. They verify a project
override, persisted user preference, native version rejection across two live
clients, reconnect rejection, cold readback, and a first save with no existing
user configuration file. The latter crosses the retained Decodex process bridge.
These tests do not use production configuration, audio or inference.

Runtime fixtures cover changed source identity, replaced transport under the same
source key, stale review tokens and failed readback. A GPUI service-socket fixture
clicks a voice and sends Space after the click; a lost save reply produces one
write and a fresh read. The refreshed effective override remains visible without
starting audio. A separate panel test checks source invalidation.

Signed desktop interaction, actual audible voice on the next call and microphone/
WebRTC acceptance remain open. These tests do not establish that qualification.

## Subtraction boundary

The picker, local query/action and conditional preference adapter form an optional
unit. The effective-voice read in the existing call path is a separate correctness
fix and can remain after the picker is removed. Keep native account ownership and
configuration version checks for any retained settings writes. The full manual
catch-up remains incomplete. Automation stays paused after delivery.

Validation results: adapter191 passed/eight opt-in; protocol136 unit plus six
integration passed; runtime584 passed/45 opt-in; GPUI467 passed/five opt-in.
Installed-native adapter and retained-bridge tests passed separately. The GPUI
fixture is a test window and local service peer, not the installed signed app.
Strict adapter, protocol, runtime and GPUI Clippy passed for all targets and features.

## Preserved media review from the inherited snapshot

The following two sections preserve the original media-owner analysis and test
limits. Their commands, log paths, pass counts and review-queue wording are
historical records, not a current execution result or a new implementation
backlog. Current source still uses WebKit track muting, bounded browser media
ownership and fixed startup-failure messages. The explicit loopback test remains
opt-in; this reconciliation does not claim that it passed. See
[the complete settings-file reconciliation](voice-settings-reconciliation.md).

## Mute and backlog review (upstream 1164)

`28f43b0417ab19632f7e53d17806b98151d6b9a0` changes the native
`voice-host` Opus pipeline. Stale or saturated media is discarded within existing
queue bounds; mute sends generated silence. Fixed failure categories avoid raw
SDP and device errors. The cutoff preserves these paths. Subsequent Bluetooth
format and Linux buffer changes remain in the source-review queue.

Decodex uses WebKit WebRTC tracks and browser-owned media queues. Its mute command
sets `MediaStreamTrack.enabled` and retains the peer. Do not copy native Opus
processing into this adapter. Dictation PCM and caption delivery are distinct
from the WebRTC media queue.

A new opt-in native test connects a synthetic sender to a local receiving peer,
then checks signal energy, packets during mute and restored audio after unmute.
Run `VoiceMediaHostTests/testWebRTCMuteKeepsPacketsFlowingAndUnmuteRestoresAudio`
with `DECODEX_NATIVE_WEBRTC_LOOPBACK=1` in a suitable interactive host. The current
host produced no local ICE candidates and did not connect. This acceptance test
has **not passed**. The ordinary media suite passes nine tests and skips this test
and the existing subscription-call qualification. Logs:
`/tmp/decodex-1164-loopback-final.log` and `/tmp/decodex-1164-media-suite.log`.
Actual audio, prolonged mute and media backlog acceptance remain open.

## Startup failure review (upstream 1172)

`da20788df913189878ebca7f4963d8a363ee6bf2` adds native failure stages and
Linux ALSA/PipeWire buffering. Decodex uses WebKit on macOS, so Linux native
buffer and packaging changes do not apply to this media adapter.

The WebKit adapter now distinguishes capture, audio runtime and connection
startup failures. It emits fixed messages without the underlying device or SDP
error. Each failure releases the peer and microphone tracks. A service closure
after a failed call retains the failure for the next client poll and does not
queue a second stop request.

`/tmp/decodex-1172-media-failure-final.log` passes a real WebKit test with three
injected failure stages and cleanup assertions. The capture case fails synthetic
stream creation; it does not test OS microphone permission.
`/tmp/decodex-1172-media-suite.log` passes ten tests and skips the two existing
native audio qualifications. `/tmp/decodex-1172-gateway.log` passes two mailbox
regressions, including failure followed by closure before the client polls.
Runtime all-target Clippy passes in `/tmp/decodex-1172-lint.log`.
The signed desktop and actual audio acceptance gaps above remain open.
