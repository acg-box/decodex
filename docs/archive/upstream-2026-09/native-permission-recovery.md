> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Native permission restoration qualification

Restore the inherited installed-native permission fixture. It selects a named
profile, an approval policy, an approval reviewer and a working directory, then
checks them after warm hydration and a cold native-process restart. The local
startup configuration deliberately differs. It must not overwrite the saved
native selection when Chief continues the task.

Installed Codex 0.158.0-alpha.2 passes both phases. The selection receipt reaches
`target_observed`; live native observations and reopened local state retain the
selected profile, reviewer and directory. The fixture makes exactly two inference
requests before restart and three in total after the requested cold continuation.
It uses private homes, temporary stores and a loopback Responses backend, with
awaited native child shutdown. No real account or production settings are used.

The complete test body is preserved. Its former HTTP helper is now shared through
`native_goal_fixture`, which contains the same retained response and gate logic.
This avoids adding a second copy of the fixture server. No production code changes
are needed for this qualification.

## Complete runtime owner disposition

`chief_permissions.rs` retains the inherited source-bound read and once-only write
path. Every difference is accounted for: persist transport-current facts before
inspection; hold editing while model or plugin operations are unresolved; let
allowed named profiles follow native running-task eligibility; classify local
queue/frame refusals as rejected; and persist configured native observations with
their transport/settings revisions. Missing current facts invalidate the saved
observation without falsely settling a request.

The service regression passes queued, rejected and uncertain results, source
changes, restored-but-stale settings, disabled profiles, foreign thread rejection,
reopen and no replay under a different request key. Strict runtime lint passes
with all features and targets. The shared database file remains open, including
its old model-recovery journal compatibility; this file disposition does not
claim that separate migration concern is resolved.

At fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`app-server/tests/suite/v2/thread_resume.rs` covers acknowledged settings and cold
permission-profile restoration. Native Codex resolves and enforces the profile.
The fixture verifies reported policy and retained selection, not every filesystem
operation allowed or denied by that profile. Signed desktop acceptance remains
separate.
