# Native permission controller qualification

Restore the full inherited permission-owner test file through the current shared
Responses helper. Keep all five scenarios, with one explicit policy adaptation:
named permission profiles are selectable during a running task in the current
implementation. Do not restore the old idle-only restriction.

## Restored coverage

- Retained wire: reject changed source and thread identity, discard a read after
  source change, reject a disallowed profile, send one queued or lost-reply edit,
  keep the receipt pending, block dispatch and reject replay with a new key.
- Idle native controller: review the named profile, queue it, observe the exact
  native publication, confirm the saved profile and reject the old review. The
  settings edit does not create a first user turn.
- Native session preservation: switch away from and back to session-defined and
  disk-defined profiles, including top-level selection. Preserve model and effort,
  keep an independent task on its own defaults and leave config bytes unchanged.
- Ordinary native resume: ignore stale client model, instructions and fast tier;
  preserve saved native settings without inference. An independent native process
  changes model and cwd. A cold reader rejects the stale cwd, then decodes the
  saved model and provider after explicit cwd reconciliation, without more input.
- Running builtin selection: leave the pending dynamic tool paused, keep its
  exact turn, observe the permission publication, retain confirmation after
  SQLite reopen, and continue only after the explicit tool response. The two
  expected model requests occur and the config file stays unchanged.

The running test now expects an allowed named profile to be selectable and omits
the obsolete rejection attempt. Its remaining builtin-selection assertions stay
unchanged. The existing separate installed-native named-profile test passes:
a running task changes its named profile and retains it across native restart.
The current service test also passes its queued, rejected and unknown outcomes,
source checks and A-to-B-to-A review invalidation.

## Evidence and limits

All five restored tests pass on Codex CLI 0.158.0-alpha.2; four use the installed
native process. The separate native named-profile test, current service test and
strict runtime all-feature/all-target Clippy pass. No production code changes.

At fixed cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, the app-server
permission update loads the thread's session configuration before queuing the
change. Its `thread_settings_update_preserves_session_profiles` test covers the
same three session/disk cases. This source was read, not executed here. The
current running-name behavior is also qualified against the installed binary.

Close this complete inherited test-file disposition only. Permission execution
and enforcement stay native. These fixtures do not qualify kernel enforcement,
real account enrollment, signed desktop interaction or the full service lifecycle.
The permission selector stays optional in the user's subtraction review; ordinary
resume preservation is core for the existing conversation consumer. Shared test
owners and the broader acceptance groups remain open. Automations stay paused.
