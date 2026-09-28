> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore native usage recovery coverage

The pre-snapshot base `2ffa385c3b49efe6a4109de0fd7353fb64abd2c5` includes
`chief_process_native_usage_tests.rs`. The current native fixture registry lost
this module. The synthetic usage replay test in the Chief suite does not replace
its installed-process restart check.

Restore the original module and its registration. Adapt only the shared Responses
fixture call to the current helper and supply its assistant output explicitly.
Keep all original assertions:

- The first turn records 10 input tokens and 2 output tokens.
- Stop the native process and reopen both Codex and the local database. Invalidate
  the disconnected local baseline before recovery.
- Recovery does not replay the first input. The second turn has a different ID.
- The cumulative counters are 30 input tokens and 6 output tokens. The second
  turn delta is 20 input tokens and 4 output tokens.
- The new thread reports its opted-in raw response. The installed binary does
  not emit a raw response after cold resume. Preserve the first response receipt
  and do not invent a receipt for the second turn.
- Exactly two Responses requests occur.

The restored test passes against installed Codex `0.158.0-alpha.2.1`, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
It uses an isolated temporary home, loopback provider and disposable SQLite store;
no real account credentials are needed. One test passes with none ignored in
0.71 seconds. The log is `/tmp/decodex-native-usage-restored.log`.
Strict stable runtime Clippy passes all features and targets in 12.71 seconds;
the log is `/tmp/decodex-native-usage-clippy.log`.

This is a coverage repair for core usage correctness. No production code changes.
The full shared native registry review remains open, including context, detail,
continuation and settings test mappings. Do not count this one restored test as
signed desktop acceptance or close the registry's inherited file row.
Automations remain paused.
