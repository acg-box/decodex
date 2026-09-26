# Native media acceptance

Classification: core compatibility for the existing timeline preview. The fixed
upstream cutoff is `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

## Relative user media

The native protocol accepts `LocalImage` and `LocalAudio` paths as `PathBuf` values.
At the cutoff, `protocol/src/local_media.rs::read_bounded_local_media` and the
local-image conversion in `protocol/src/models.rs` open those paths directly.
Relative paths therefore use the native process directory. A child thread's
configured directory is not a substitute for that directory.

Timeline preview previously rejected all relative paths. The service now resolves
relative local input against the directory retained for the exact admitted process
generation. It uses the same process owner as prompt editing. No UI-supplied base
path or current service directory is used. Missing or nonabsolute process directories
remain unavailable. Absolute paths and native inline bytes retain their existing
behavior. Account, generation, thread, history and credential changes still discard
the result after the read. Continuation fingerprints bind the content and source.

This reads current local file contents, not a guaranteed historical snapshot. Native
inline image results retain their exact bytes. An executor imageView path or an
imageGeneration savedPath alone does not establish a service-host file identity;
those paths remain unsupported. Stored file IDs and remote URLs do not authorize a
new private file-download or network-fetch mechanism.

## Remaining acceptance

The pre-change focused suite passed 27 tests for media, attachment pagination,
external context and real desktop Preview clicks. Nextest reported one leaky-handle
warning for the duplicate attachment-page test; this was not a leak-free run.
The isolated attachment-page rerun passed without a leaky-handle warning.
The nine media tests passed after the change, including exact native-history reads
across source changes and missing or invalid directory evidence. Runtime all-target
Clippy passed with `-D warnings`.

## Installed-native and public service qualification

Installed Codex 0.158.0-alpha.2 retained exact localImage/localAudio relative paths
with a process directory different from the thread directory. A synthetic model
catalog qualified both audio support and native omission for a model without audio
support. Both cases used one local model request; no real account was involved.

The existing isolated account/runtime/socket fixture now accepts DECODEX_TEST_MEDIA=1.
It starts the real production service and installed Codex, then changes only the
native thread directory. It verifies the retained process directory explicitly.
Public ChiefClient media reads return the exact image and 64,044-byte WAV, with the
WAV spanning two chunks. A foreign work ID is refused and reads add no model request.
The fixture passed, as did strict Runtime Clippy. The initial fixture incorrectly
started the process in the alternate directory; it was corrected before acceptance.

R04 remains open until final desktop consumer acceptance is complete. The current
desktop Preview control renders images; service audio reads do not establish an
audio playback UI.
Interactive MCP App UI remains the separate optional R05 capability. This record
does not establish signed whole-app acceptance, installation or full catch-up.
