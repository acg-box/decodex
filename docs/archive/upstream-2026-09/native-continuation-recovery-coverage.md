> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore native continuation restart coverage

Restore `chief_process_native_misalignment_tests.rs` byte for byte from
pre-snapshot base `2ffa385c3b49efe6a4109de0fd7353fb64abd2c5` and register it in
the current native fixture module. No production code changes.

The existing `chief/tests/native_misalignment.rs` verifies live confirmation,
stale-token refusal and a single continuation through a directly spawned client.
It does not cover the retained bridge, override metadata or process restart.
The restored test checks those additional boundaries:

- Read the explanation and steer from the live native error. Historical reads
  omit the explanation and do not cause model work.
- Reject a stale confirmation token without another provider request.
- Send an explicitly confirmed continuation with the native override timestamp
  and steer input. Clear the completed precaution.
- Leave a later turn blocked, stop the process and start a new process.
- Historical state remains readable, but the new connection has no live review
  authority. Reject the old token without an additional provider request and
  retain the unresolved precaution.

The original test passes against installed Codex `0.158.0-alpha.2.1`, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
It uses a temporary home, a loopback synthetic provider and a disposable database.
One test passes with none ignored in 0.70 seconds. The log is
`/tmp/decodex-native-continuation-restored.log`.
Strict stable runtime Clippy passes all features and targets in 12.11 seconds.
The log is `/tmp/decodex-native-continuation-clippy.log`.

This restores coverage for the existing native continuation contract. It does
not add a local policy engine, qualify signed desktop interaction or close the
shared native fixture registry review. Automations remain paused.
