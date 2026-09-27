# Restore native patch paging coverage

Restore `chief_process_native_detail_tests.rs` from pre-snapshot base
`2ffa385c3b49efe6a4109de0fd7353fb64abd2c5` and register the fixture. Adapt only
the renamed shared Responses helper and its absent body-capture argument.
Keep every original assertion. No production behavior changes.

The native fixture creates a patch with 2,500 Unicode lines in a temporary
workspace. After native completion, it verifies the file and reads the complete
patch through the existing detail owner:

- The reconstructed text exceeds 64 KiB and includes every Unicode line and the
  required final line.
- Each page is at most 8 KiB, uses the exact byte offset and has a consistent
  continuation marker. Paging must terminate.
- After native process restart, the old process-bound cursor is unavailable.
  A fresh cursor reconstructs the same complete text.
- Exactly two provider requests occur. Reads and restart do not run inference.

This completed-patch history check is different from pending approval evidence.
The [live file approval test](runtime-evidence-owner-reconciliation.md) requires
that the pending diff is retained before native history contains it. Keep both
checks; neither substitutes for the other.

The installed-native test passes on Codex `0.158.0-alpha.2.1`, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
One test passes with no skips in 0.79 seconds. The log is
`/tmp/decodex-native-patch-restored.log`. The test uses an isolated temporary home
and loopback provider without account credentials.
Strict stable runtime Clippy passes all features and targets. The log is
`/tmp/decodex-native-patch-clippy.log`. A source comparison confirms that only the
shared helper adaptation differs from the original fixture.

All four lost pre-snapshot fixture modules now have restored installed-native
coverage: usage, continuation authority, tool context and patch detail. The
shared registry review remains open for other inherited inline tests and helper
changes. Signed desktop acceptance remains separate. Automations stay paused.
