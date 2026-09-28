> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Restore native context restart coverage

Restore the context fixture from pre-snapshot base
`2ffa385c3b49efe6a4109de0fd7353fb64abd2c5` and register it with the retained
native bridge. Adapt the renamed shared Responses helper and provide its output
explicitly. Keep every original assertion. No production behavior changes.

The existing local external-context tests verify outgoing requests. The existing
manual native delegation test does not cover the complete automated restart
sequence. The restored test verifies these boundaries together:

- An external result enters the model request as a named `decodex` tool output,
  with no fabricated call ID and no user-role promotion.
- A duplicate external result does not start another turn.
- Initial and follow-up worker instructions remain tool-owned in native history.
  They do not become user messages.
- Public history retains tool provenance and the exact native response amount.
- After both Codex process restart and database reopen, task histories remain
  equal and recovery does not replay work. The provider request count stays six.

The installed-native test passes on Codex `0.158.0-alpha.2.1`, SHA-256
`3e11ccc743e8198a5ef84fb57c89941d845b0ea0302485ed1fbac2f0821aca5a`.
One test passes with no skips in 1.19 seconds. The log is
`/tmp/decodex-native-context-restored.log`. The fixture uses a temporary home,
loopback provider and disposable database without real account credentials.
Strict stable runtime Clippy passes all features and targets in 13.25 seconds;
the log is `/tmp/decodex-native-context-clippy.log`.

Keep the remaining native fixture registry review open. This qualification does
not establish signed desktop behavior or enable scheduled work. Automations stay
paused.
