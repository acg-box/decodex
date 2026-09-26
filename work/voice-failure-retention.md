# Preserve voice failures through cleanup

The signaling mailbox replaced a failed call with a normal ended state when
cleanup completed before the desktop polled. This also discarded the actionable
error message. Restore the original terminal update behavior: an ended update
marks cleanup complete, clears any answer, and retains an existing failure and
its message. A later stop or expiry must not enqueue another cleanup request.

The current Chief voice actor maps `thread/realtime/error` to failed and
`thread/realtime/closed` to ended. With an established answer, these notifications
can reach the same saved call in that order. Fixed upstream commit
`595cc91e8cbb1c2ca822d0311dcf12709410c582` defines these separate error and closed
notifications in `codex-rs/app-server-protocol/src/protocol/v2/realtime.rs`.
The installed Codex 0.158.0-alpha.2 schema includes both.

## Evidence and scope

All 20 local voice tests pass; two existing external tests remain ignored.
Strict runtime lint passes with all features and targets.

The restored regression failed before the fix: a poll returned `Ended` instead
of `Failed`. It checks the retained error message, no duplicate stop and no
expiry cleanup. The complete `chief_voice.rs` now matches its verified preserved
snapshot. This is a local signaling-state fix; it does not prove microphone,
WebRTC media, live-provider or signed desktop acceptance. The independent audio
loopback limitation and final voice acceptance remain open.
