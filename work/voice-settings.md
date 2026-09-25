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

The adapter binds the reviewed object to its native connection. Protocol2.82 adds
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
