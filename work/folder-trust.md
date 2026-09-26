# Folder trust and native resume

## Current qualification on 2026-09-26

Native Codex owns directory trust. Decodex does not add a trust editor or grant
trust automatically. Restore the complete inherited native trust fixture through
the retained bridge, in `chief_process_native_folder_trust_tests.rs`. Its only
behavioral change from the preserved fixture is explicit native-session shutdown
before stopping the synthetic backend.

Installed Codex 0.158.0-alpha.2 passes the fixture: trusted project settings apply;
untrusted and unknown project layers stay disabled; repeated reads do not write
trust; revocation affects a fresh config read while a loaded task retains its
settings. There is exactly one explicit inference and no resume replay.

The review also found a missing ordinary resume contract. Merely omitting model
and tier did not prevent stale creation cwd and developer instructions from being
written back. Restore `inherit_native_settings` in the typed request and use it
for runtime same-thread recovery. The wire contains only `threadId` and
`excludeTurns`. Exact response thread and expected directory checks remain.
Creation and explicit turn submission retain their existing per-field overrides.
This preserves the optional ordinary History consumer; it does not add navigation.

The fixed-cutoff `ThreadResumeParams` schema identifies model, cwd, tier and
instructions as optional overrides. Its persisted-collaboration resume test keeps
saved directory, effort and instructions when they are omitted. These upstream
sources were read, not executed. The cutoff remains
`595cc91e8cbb1c2ca822d0311dcf12709410c582`.

A runtime wire regression fails before the fix and passes afterward. All 23
conversation adapter tests and 16 runtime conversation tests pass. Strict adapter
and runtime lint passes. The separate production native cold-runtime fixture
passes creation, same-thread continuation, service restart, warning persistence
and exact request counts: four explicit inference requests and no recovery replay.
It uses the established private-home harness and synthetic credentials. An initial
run under `/private/tmp` failed at cold metadata admission before it reached the
changed resume path; that run is not passing evidence.

The source, fixture and historical documentation are reconciled here. Complete
review of the larger runtime/process modules and signed desktop interaction is
still open. Automations remain paused. The earlier review below is historical;
its former claims about delivery are superseded only by the current evidence above.

## Preserved earlier review

Upstream commits `84e7d4a1fefd0c595c29cdb9af6c065fc7277d98` (1176) and
`02a8f038b87ad34d4a1dc5058eda26972ed7aa6c` (1180) check the resolved task
destination before startup, creation and resume. Their complete patches were
read. The final trust lookup keeps canonical path precedence, explicit child
choices, project markers and Git roots. Connected local resumes also check the
saved folder and reread it after consent. Remote scope remains explicit `--cd`.

## Verified native behavior

The installed-native test
`installed_native_config_reads_preserve_folder_trust_without_granting_it`
uses the retained Decodex bridge, an isolated home and a local Responses fixture.
It verifies these facts:

- Trusted project configuration applies. Untrusted and unknown project layers
  are disabled. Switching directories does not reuse the previous decision.
- Configuration reads do not write trust.
- After one completed turn, revoking trust disables the project layer for a new
  configuration read. Resuming the loaded task retains its prior reasoning
  setting. The resume does not dispatch another model request.

Evidence: `/tmp/decodex-1176-native-resume-materialized.log`, one passing test.
The initial empty-thread resume fixture was rejected before materialization;
those failed attempts do not prove resume behavior.

## Applicability

Decodex does not currently write folder trust through its task UI. Chief restores
native settings without applying creation defaults; ordinary resume also uses
`inherit_native_settings`. This preserves native tasks, but a current directory
configuration read cannot establish what an existing task loaded earlier.

The TUI commits add a consent screen around the TUI's own trust mutation and
navigation. Decodex does not offer that mutation or claim to reproduce the TUI
onboarding. It leaves native trust unchanged and passes the actual selected
working directory to the native owner. Existing tasks retain their native
settings. The permission panel displays observed task facts and the actual
native directory, with a source guard; it does not label a task restricted from
the folder's current trust value. Do not add an automatic trust grant or silently
reset task settings.

The relevant integration defect was the handling of native configuration
warnings during task establishment. That defect is fixed below. The source
applicability review for 1176 and 1180 is complete. This does not certify signed
desktop acceptance or add a folder trust editor to Decodex.

## Ordinary diagnostic handling

Source inspection found a separate integration defect: ordinary thread creation
and resume reject unexpected execution events, but the event list also contained
display-only native warnings. A valid configuration warning could therefore
turn a successful operation into an ambiguous result.

The process adapter now retains warning events separately for both operations.
Execution events remain in their receipts, preserving the existing protection.
The next owned turn receives retained notices before its response events, even
when the turn completes immediately. Failed resume attempts retain their events
for the next observation. The production permission panel already binds its
facts to the exact native task and directory; it does not infer task restrictions
from folder trust.

A fake process regression exercises both warning methods and two resumes, then
checks exact-once notice delivery. The existing closing-resume test still checks
that execution events remain visible. Installed-native application qualification now passes in
`/tmp/decodex-1176-native-app-final.log`. The test uses production account/process
admission, explicit model review, ordinary thread creation, one local Responses
request, completion publication, shutdown and reopened history projection.
A project-level unknown key produces exactly one status notice attached to the
user turn. Its private value is absent. Retried confirmation does not submit
another inference request. The initial test requested more history rows than
the protocol allows; the corrected test uses the public limit and requires no
unread history page. Signed desktop interaction remains a separate open check.
