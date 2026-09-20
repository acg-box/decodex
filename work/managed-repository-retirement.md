# Managed Repository retirement

This change removes the obsolete Managed Repository capability from main
`be1398c21fba995569f8f6a02a7ba51eea6cb7b5`. It is isolated on
`xv/decodex-retire-managed`, separate from the earlier uncommitted broad refactor.

## Removed scope

- Core allocation, admission, Git registration, worktree readiness, commit,
  reconciliation, evidence, and operation-assignment state machines and exports.
- The disabled runtime capability, readiness facade, bootstrap construction, and
  unused application field and constructor argument.
- The doctor component and its CLI and desktop presentation.
- Four dedicated tests of the retired state machine. Their owner has no supported
  execution path. Current doctor-report, conversation, context, and supervised
  validation tests remain.

No current database table or migration belongs to this capability. No user Git
repository, worktree, branch, credential, application database, or recorded
conversation is changed or deleted.

## Retained independent value

`RepositoryContentRevision` is still used by context revision and supervised
validation. It now lives in `repository_revision.rs`, with its own narrow error.
It remains an opaque, exact, nonempty string of at most 256 UTF-8 bytes, with no
leading/trailing whitespace or control characters. A regression test covers exact
roundtrip, boundary size, Unicode byte limits, whitespace, newline, and NUL.
This value grants no Git mutation authority.

## Protocol and documentation

The closed doctor component set changes from 19 to 18. Exact protocol 2.41 replaces
2.40; negotiation explicitly rejects 2.40. App and service must be updated together.
The repository's exact-version boundary remains intact. JSON golden fixtures and
CLI count expectations match the new report. README records the retirement.

At the user's explicit request, OpenWiki current guidance was corrected and obsolete
repository execution requirements were removed. Frozen historical evidence retains
recorded identities and is explicitly labelled archival, not current authority. Production source,
configuration, and current database sources have no retired capability references.
Negative architecture assertions retain retired names to prevent their return.

## Validation

Evidence is stored under `target/managed-repository-retirement/`. The worktree
uses the prior task-owned Cargo target directory as a build cache. Test commands
run against this worktree's source and revision.

- Full workspace check: passed for all targets and features.
- Full Rust nextest: 1,550 passed; 18 skipped. Two tests reported stdio-leak
  markers; all three corresponding executions passed a serial recheck without
  leak markers. Follow-up evidence is kept separately.
- Current vNext architecture: 16 passed, including removal and version checks.
- Real CLI diagnostic process tests: two passed.
- Strict lint: 11 packages passed. GPUI failed at an unchanged wildcard import
  in `src/window_material.rs:60`, also compiled into the capture binary.
- Repository formatting: failed only in five unchanged files:
  `chief_async_questions.rs`, `chief_voice.rs`, runtime `chief/tests.rs`, and
  database `lib.rs` and `migrations.rs`. Changed Rust files passed formatting.
- SQLite gate: its unchanged migration list has 27 entries while the unchanged
  database migration owner is schema 28. The gate's version-count assertion fails.
  Database tests and initialization/validation commands ran; no schema was changed
  by this retirement.
- The additional unchanged login architecture suite retains four stale assertions:
  it forbids existing `reqwest`/`time` dependencies and requires fully qualified
  `AccountLoginRequest` variants where source now uses `Self`.
- `git diff --check`: passed.

The listed baseline source files were compared byte-for-byte with the base commit.
Their failures are not counted as passing validation. No baseline checks or tests
were disabled. This change is not committed, merged, installed, or released.

## GitHub effect retirement

The subsequent user-authorized cleanup removes the disconnected GitHub effect
module, its five dedicated tests, module declaration, and dormant-code exemption.
The deleted implementation was 1,835 lines; its tests were 535 lines. It had no
production caller, concrete provider adapter, or dispatch-receipt issuer.

A reverse scan of 67 domain-specific identifiers found no consumers in application,
runtime, database, tests, scripts, configuration, or automation sources. Existing
Radar, Publisher, and repository maintenance automation remain independent and
unchanged. Shared dependencies are still consumed by live runtime modules; none
was removed merely because the deleted module imported it. No additional protocol
or database change is required for this private, unreachable module.

README records the retired built-in PR/check-run workflow. OpenWiki current guidance is corrected; frozen historical evidence is explicitly
labelled archival.
Fresh runtime regression, strict runtime lint, and workspace compilation logs use
the `decodex-retire-github-` prefix in the evidence directory.

GitHub retirement validation: 441 runtime tests passed, seven skipped; strict
runtime lint and all-workspace/all-target/all-feature compilation passed. No
stdio-leak marker appeared in the runtime run. `git diff --check` passed.
