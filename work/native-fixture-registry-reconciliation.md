# Reconcile inherited native fixture registration

Compare the complete inherited `chief_process_native_tests.rs` diff. Restore
lost assertions and identify moved tests before closing this source row.
The fixed upstream cutoff remains
`595cc91e8cbb1c2ca822d0311dcf12709410c582`.

## Configuration coverage

Restore three inherited inline tests in `chief_process_native_config_recovery_tests.rs`:

- Connection settings: create the absent config file, preserve an escaped Unicode
  link identity, reject an old file version, retain the saved preference after
  restart, reject an old connection and remove only the selected override.
- Login policy: absent restrictions remain absent; disk edits cannot change the
  running policy; restart observes the changed API/ChatGPT restriction in both
  directions. This complements the existing prohibited-login enforcement test.
- Initial Daybreak preference: retain true, false and omitted values on thread
  start and read. Reject an explicit preference for an ephemeral thread. This
  does not grant model access or exercise a real account entitlement.

The connection writer now returns an acknowledgement separate from settings.
Read settings explicitly after each successful write and check that its version
matches the acknowledgement. Keep the original effective and user-value checks.
The initial direct restoration failed to compile because it used the removed
`settings` receipt field; preserve that log at
`/tmp/decodex-native-config-restored.log`.

Extend the existing retained-bridge voice test instead of duplicating it. Restore
the catalog assertion, same-connection stale-version rejection and effective
voice readback after restart. Keep the existing absent-file and old-connection
rejection checks. No production code changes.

## Complete registry mapping

| Inherited difference | Current disposition |
| --- | --- |
| Four removed base modules | Restore installed-native usage, continuation, context and patch fixtures. See [usage](native-usage-recovery-coverage.md), [continuation](native-continuation-recovery-coverage.md), [context](native-context-recovery-coverage.md) and [patch paging](native-patch-recovery-coverage.md). Each retains its original assertions and has a passing installed-native run. |
| Voice, connection, login and Daybreak inline tests | Restore or extend the current fixtures as described above. |
| MCP discovery, permission catalog and enterprise MCP | Move to the registered discovery module. Function bodies match the preserved originals. |
| Model access and summary history | Move to their registered modules. Function bodies match the preserved originals. |
| Folder trust | Move to the registered folder-trust module. The only body change explicitly drops the native session before aborting the local backend. |
| Responses helpers | Consolidate body capture, usage, text, output and SSE frame helpers under `serve_fixture`, `serve_fixture_usage` and `serve_fixture_frames`. Keep synthetic usage amounts and request capture. Support output arrays and optional effort/turn metadata checks. Restored fixtures exercise these shared paths. |
| Session initialization | Use explicit experimental API support and omit raw response-item notifications in this isolated native test client. This is fixture configuration, not a claim that every production initialization field is identical. Keep environment clearing, temporary home and child cleanup. |
| Other module registration | Retain current model, plugin, permission, reviewer, compaction, audio and related fixtures. Module order and crate visibility do not remove their registration. |

The remaining known explicit Flex cold-resume and child MCP failures retain
their separate qualification records. Source reconciliation does not convert an
ignored test or a failed native capability into a pass.

## Validation and scope

All three restored configuration tests pass against installed Codex
`0.158.0-alpha.2.1` with no skips in 0.27 seconds. The extended voice test passes
with no skips in 0.18 seconds. Logs are `/tmp/decodex-native-config-adapted.log`
and `/tmp/decodex-native-voice-config-restored.log`. The binary SHA-256 is
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
These fixtures use isolated temporary homes and no real credentials or login.
Strict stable runtime Clippy passes all features and targets in 12.89 seconds;
the log is `/tmp/decodex-native-config-clippy.log`. Formatting and diff checks pass.

Close only the shared native fixture source row after the full mapping and
snapshot hash verification. Native bridge implementation, separate qualification
rows and fresh signed desktop acceptance remain open. Connection, voice and
model-access controls remain optional for the user's removal decision; the
retained controls must preserve their native version and identity rules.
Automations remain paused, including after completion.
