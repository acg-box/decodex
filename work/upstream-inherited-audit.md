# Inherited change reconciliation

Audit baseline: Decodex `7410a97b690f95a8567253f9833a9d47d436d1dd` (PR1507).
The preserved working-tree snapshot started from
`2ffa385c3b49efe6a4109de0fd7353fb64abd2c5`. It contains 357 files and three
deletions. All 357 file SHA-256 values still match the takeover manifest.

This audit covers inherited files, not the 1,569 upstream commits. File counts do
not measure feature completion. A merged capability can touch many files, and
one shared file can contain both delivered and outstanding behavior.

## Restored owners reconciled after PR1544

Nine complete-file comparisons were verified at
`e52af9dbabcbf48ab4536306a79c4c338531b6b8`. Every original snapshot hash matches
its register entry. The register refreshes each current path and owner hash.

| Original path | Complete disposition and delivery evidence |
| --- | --- |
| `crates/decodex-codex/src/lib.rs` | Exact snapshot bytes after PR1544 restores the native provider bound export. |
| `database/migrations/0038_conversation_native_settings.sql` | Exact snapshot bytes at registered migration 47 after PR1544. The original path remains absent; existing migration history was not rewritten. |
| `database/src/conversations/native_settings.rs` | Only the effort bound changes from 32 to the current protocol's 128 bytes. PR1544 verifies source checks, response ordering, preserved intent, durable readback and migration upgrade. |
| `crates/decodex-runtime/src/chief/tests/result_integrity.rs` | All inherited assertions remain. PR1541 runs the original large-result case with an item ID and the additional no-ID case. PR1531 already restored native terminal timing. |
| `crates/decodex-runtime/src/chief/guardian.rs` | Resume now uses the canonical native settings parameters and response-observation owner. PR1542 verifies unloaded-thread approval without startup overrides, new turns or changed denial evidence. See `guardian-large-observations.md`. |
| `crates/decodex-protocol/src/chief_timeline.rs` | Adds only the defaulted `app_ui` metadata flag from PR1521 and clarifies Summary comments. Summary wire fields are unchanged; PR1528 restores its consumer. |
| `crates/decodex-runtime/src/account_launch/chief_process_app_exposure_tests.rs` | Two explicit imports and shared App configuration receipt calls replace the retired per-app receipt API. All other assertions remain. The focused host fixture passes; PR1480/PR1481 and `app-tool-exposure.md` document the canonical journal. |
| `apps/decodex-gpui/src/chief_app_exposure_tests.rs` | The fixture observes ChiefSurface and renders its integration panel through an explicit view. All test actions and assertions remain. |
| `apps/decodex-gpui/src/chief_app_exposure_wire_tests.rs` | The same explicit panel fixture replaces the old whole-surface render. Lost-reply and single-send assertions remain. Both GUI files passed in the latest full GUI run. |

This batch closes nine content-review rows. Its baseline has 204 pending rows;
the standalone batch leaves 195. Separately submitted batches can reduce that
count further. All 360 entries remain. Complete shared-file, native and signed
desktop acceptance boundaries are unchanged. No production code changes in this
audit, and prior executable checks are not repeated for unchanged files.

## Stored tool image references restored on 2026-09-26

`crates/decodex-runtime/src/chief/timeline/attachments.rs` now matches the complete
preserved snapshot. A valid native `file_id` image keeps its stored-image source
and content index without exposing the reference. Empty and malformed IDs still
produce an unknown descriptor. The regression failed before the fix; all 38
Chief timeline tests and strict runtime lint pass. See
[Stored tool image references](stored-tool-images-recovery.md).

The original snapshot hash matches. This batch closes one file and retains all
360 register rows. Its main baseline has 193 pending rows; this isolated change
reduces that count to 192. Other pending batches close separate rows. No shared
desktop acceptance group is closed by this change.

## File evidence

[The complete 360-row register](upstream-inherited-files.tsv) contains hashes
from the committed audit baseline, with the three dependency rows refreshed at
`943eb039ddf785fd99200326180887bcc657fa7e` and nine source rows refreshed at
`63f3b110afb6f40fb55cbd46d1ef9ac13acd56a9`, plus the four source rows described
below at `10ccb2bd3e44a29d39e23d521c44ff2e34807916`. It separates direct content comparison from
recorded adaptation decisions. The owner hash refers to current_owner; the current
hash always refers to the original snapshot path. This distinction matters when a
migration number now names a different migration.

| Directly verified relationship | Entries |
| --- | ---: |
| Exact content at the original path | 86 |
| Matching deletion | 3 |
| Exact migration content at a different registered path | 3 |
| Non-identical content requiring adaptation evidence or residual review | 268 |

Raw path comparison: 86 exact files, 187 different files, 84 absent paths and three
matching deletions. All 357 preserved file hashes still match the takeover
manifest. The three migration mappings were checked against the migration registry:

| Snapshot migration | Current registered migration |
| --- | --- |
| 0037_chief_async_skips.sql | 0034_chief_async_skips.sql |
| 0039_chief_reasoning_summary.sql | 0041_chief_reasoning_summary.sql |
| 0041_chief_request_payloads.sql | 0039_chief_request_payloads.sql |

The 268 non-identical rows are not 268 missing capabilities. Their
recorded_disposition column preserves earlier delivery decisions and partial PR
references. The previous task-local ledger had 239 rows labeled
requires-content-review; it also classified adapted files and historical
successors. These counts answer different questions. Neither is a feature
completion percentage. A partial delivery does not close a shared file, and an
absent old path does not prove that its behavior is absent.

The original PR1378 remains open. Both stash objects remain present:

- `9187391b7ffc569f8d304ae3bfdfea5c32d566cc`
- `3a9454d5457882473cb49697872ea6403ecf4b29`

Do not close the original PR or remove the snapshot/stashes from these counts.
They prove preservation, not complete integration.

## Recovery and draft owners reconciled after PR1547

Eight complete-file comparisons were verified at
`eaba4009094d203d28ef39422d6baa51accd379e`. All original snapshot hashes match.

| Original path | Complete difference and retained owner |
| --- | --- |
| `crates/decodex-protocol/src/conversation_native_settings.rs` | Exact snapshot bytes after PR1545. Native and local projection evidence is recorded in `ordinary-native-settings-recovery.md`. |
| `crates/decodex-runtime/src/chief/turn_execution.rs` | Exact snapshot bytes after PR1547 restores typed capacity supersession. The regression failed before the repair; all 29 capacity-filtered tests pass. |
| `database/src/conversations/resume_rejection.rs` | Only variant order, comments and two refusal messages differ. Serde snake-case names and every persistence transition remain unchanged. See `ordinary-native-non-submission.md`. |
| `database/src/chief_dispatch_rejection.rs` | Adds settings-changed, request-too-large and queue-full refusals and exposes the existing note within the crate. The old refusal transitions are unchanged. `chief.rs::unsent_request_refusal` classifies only proven pre-write failures; `chief/tests/unsent_input.rs` checks that prior effects and uncertain transport failures remain fenced. |
| `database/src/provider_attempts.rs` | Only adds a read-only positive-evidence check. `application_turn_outcomes.rs` reads the existing attempt, checks its consumer, then validates durable terminal evidence. Four focused outcome tests pass, including database reopen, foreign consumer and a deleted evidence row. No execution journal is added. |
| `apps/decodex-gpui/src/chief_drafts.rs` | Adds submission-state grouping, creation-setup retention and first-profile adoption of ordinary drafts; the effort-intent test now passes its context. All inherited draft handling remains. Creation setup has cold-reopen tests and ordinary drafts keep the existing store. |
| `apps/decodex-gpui/src/chief_draft_recovery.rs` | Replaces the narrow uncertainty flag with the shared complete delivery predicate, retains queued input during cancellation, and merges unbound ordinary drafts under their own baseline check. The predicate covers pending prompt edits and unconfirmed ordinary commands. All other source is unchanged. |
| `apps/decodex-gpui/src/shell_recovery_actions.rs` | Adds only the account DTO required by the account-control fixture. The original actions and assertions remain, and the fixture cannot contact a real service. |

The focused draft run passes 64 tests. The unchanged creation-setup and account
fixture paths also passed the preceding full GUI run. This batch closes eight
rows: its standalone baseline changes from 202 to 194 pending rows, before other
pending batches. All 360 entries remain. File reconciliation does not close the
remaining signed desktop, live-provider or shared-file acceptance work.

## Dependency rows reconciled on 2026-09-26

The three dependency rows were rechecked at
`943eb039ddf785fd99200326180887bcc657fa7e`. Their snapshot and current SHA-256
values match the recorded evidence. Complete parsed TOML comparisons prove:

| File | Disposition and evidence |
| --- | --- |
| `Cargo.toml` | Semantically equal. Only the position of `unicode-width` differs. |
| `Cargo.lock` | All 800 resolved package identities, sources and checksums match. The only graph difference is the added `decodex-account-login` dependency on `core-foundation 0.10.0`. Removing that edge makes the complete parsed files equal. [PR1411](https://github.com/acg-box/decodex/pull/1411) provides the manifest dependency and macOS proxy consumer. |
| `apps/decodex-gpui/Cargo.toml` | Complete parsed equality after removal of the added `NSDate` and `NSRunLoop` features. [PR1521](https://github.com/acg-box/decodex/pull/1521) uses them to service native callbacks. |

The implementation commits `69754d2658` and `8def1e392c` are ancestors of the
checked revision. These three rows have no remaining file-level difference that
requires a decision. This closes three entries from the historical 239-row
`requires-content-review` set. At that stage, 236 entries in that set still needed
review; the source review below resolves nine more. Other previously partial
dispositions also remain open. The original
byte-comparison counts above are unchanged: structural equality is not byte equality.

Risk coverage: structural reconciliation only; no advisory or runtime dependency
risk assessment. Risk delta: no new package identities, sources or checksums in
these inherited differences. Decision: retain the delivered dependency changes.
No manifest, lockfile, dependency version or application code changed in this audit.

## Nine source rows reconciled after PR1523

The following complete-file comparisons were rechecked at
`63f3b110afb6f40fb55cbd46d1ef9ac13acd56a9`. Snapshot and current SHA-256 values
match the register. These nine rows now have explicit dispositions.

| Source file | Complete difference and retained owner |
| --- | --- |
| `crates/decodex-core/src/path_unix.rs` | One doc comment was removed. All executable source is equal. |
| `crates/decodex-codex/src/app_server_client/permissions.rs` | One doc comment now distinguishes sandbox projection from full named-profile filesystem rules. All executable source is equal. |
| `crates/decodex-protocol/src/chief_permissions.rs` | One doc comment no longer says named profiles require idle. All executable source is equal; this row does not establish runtime selection policy. |
| `database/src/chief_turn_execution.rs` | Three doc comments were added. All executable source is equal. |
| `database/src/chief_process/tests/plugins.rs` | The fixture converts `Option<&str>` to a vector with `map` instead of iterator collection. Both produce `[]` for `None` and `[p]` for `Some(p)`. All other source is equal after whitespace normalization. |
| `crates/decodex-runtime/src/account_launch/api_reset_card/tests.rs` | Two constant/default struct fields moved within the same initializer. All other source is equal. |
| `crates/decodex-runtime/src/chief_model_settings.rs` | The inherited file remains an exact prefix. A `cfg(test)` module registration was added. Its tests exist and passed in the PR1523 Runtime run. |
| `crates/decodex-runtime/src/account_launch.rs` | Only the `activation_policy` module and its export were added. [PR1428](https://github.com/acg-box/decodex/pull/1428) supplies the module and the quota activation consumer. Commit `95a898289` and merge `1aa0357c6` are ancestors of the checked revision. |
| `apps/decodex-gpui/src/chief_app_exposure.rs` | The shared configuration write label and three outcome labels were added or clarified. All controls and existing outcome branches remain. [PR1481](https://github.com/acg-box/decodex/pull/1481), commit `6791a8ba9` and merge `b67449a14`, delivers these labels. |

There are now 227 rows with `requires-content-review`, down from the historical
239 after three dependency and nine source dispositions. This is a file-review
count, not a feature completion percentage. Partial dispositions outside this
set remain open. These comparisons do not establish new runtime or visual acceptance.

A same-formatter comparison also checked 149 existing Rust files in the pending
set at the earlier `4f5bba9bd` revision. All parsed, but none became equal from
formatting alone. The register does not close files based on that check.

Guardian observation retention remains a separate open delivery at this snapshot.
[PR1524](https://github.com/acg-box/decodex/pull/1524) addresses the decoder and
store limits; its pending status does not close the inherited Guardian row here.

## Core gaps resolved since the earlier audit

The earlier baseline was 3f131d80b9e90d2badf2394249bbf3b0266f72d3. Its three core
gap rows are now delivered. Keep the remaining shared-file review separate.

| Earlier gap | Current owner and delivered behavior | Merged PRs |
| --- | --- | --- |
| Native positive pre-dispatch refusal | Chief InputNotSent and ordinary non_submission retain exact positive refusal evidence. Unknown delivery stays fenced; rejected input is not automatically replayed. | [1488](https://github.com/acg-box/decodex/pull/1488), [1489](https://github.com/acg-box/decodex/pull/1489) |
| Claimed capacity retry refusal | Registered migration 0038_chief_dispatch_refusals permits claimed cancellation only with an exact resolved refusal receipt. It supersedes inherited migrations40/42. | [1488](https://github.com/acg-box/decodex/pull/1488) |
| Large native approval payloads | chief_request_payload stores immutable complete payloads; source-bound page assembly, decision reads and live file evidence use exact request identities. Raising the inbox limit alone was not the delivered solution. | [1490](https://github.com/acg-box/decodex/pull/1490), [1491](https://github.com/acg-box/decodex/pull/1491), [1492](https://github.com/acg-box/decodex/pull/1492), [1493](https://github.com/acg-box/decodex/pull/1493) |

See [Chief refusal adaptation](native-dispatch-refusals.md) and the corresponding
PR validation records. This refresh verifies committed owners and merge ancestry;
it does not rerun application tests or claim signed desktop acceptance.


## Optional and separately reviewed work

The preserved notes describe voice preferences, public reasoning summaries,
model-access metadata, account analytics, MCP App widgets, earlier-prompt editing
and task recaps. These are separate product or presentation scopes; scanning their
upstream commits did not deliver them. Their inherited owners and current
alternatives still require reconciliation. Voice preferences, public reasoning
summaries, model-access observation and manual recaps have since been delivered
in focused batches; see the [adoption register](upstream-adoption-review.md).
Existing quota windows are not the full analytics report contract, and existing native revert observation
is not an edit-earlier-prompt action.

For this manual fixed-cutoff pass, the user authorized completion followed by a
product subtraction review. Future optional additions require user selection.
The scheduled automation remains paused, including after manual completion.

## Validation boundary

This documentation refresh checked snapshot hashes, current committed bytes, all
three registered migration mappings, current refusal/payload owners, stash
identities and GitHub PR state. No application code or production data changed. The register deliberately leaves uncertain rows open.

## Four additional source dispositions on 2026-09-26

These four rows were compared in full at
`10ccb2bd3e44a29d39e23d521c44ff2e34807916`. The preserved hashes match the
register. Each row now records the current content hash and its disposition.

| Original path | Complete difference and disposition |
| --- | --- |
| `database/src/chief_guardian.rs` | Exact snapshot bytes after PR1524. The shared 8 MiB native observation bound is restored. This closes this file, not all Guardian UI or live-provider acceptance. |
| `apps/decodex-gpui/src/chief_timeline_inputs.rs` | Only a test fixture differs: new history DTO fields use empty values, and the removed receipt-level voice-session field is omitted. All production input projection code is identical. |
| `apps/decodex-gpui/src/chief_usage_estimates.rs` | Only the visual test differs: it opens the current agent-settings menu and waits for the existing transition before checking layout. Production estimate behavior and the original assertions remain. |
| `apps/decodex-gpui/src/chief_task_references.rs` | The function body is identical. The old test-or-capture compilation guard is removed so the existing workspace fixture call can compile in the shared module. No inherited behavior was deleted. |

The register still has 360 rows. `requires-content-review` decreases from 227 to
223. Other partial dispositions remain open. Summary recovery in PR1528 is not
included in these four closures while that PR awaits merge. These are source
reconciliation decisions, not new feature adoption or an overall completion rate.

## Context and fixture dispositions on 2026-09-26

Four more rows were compared in full. Their current content matches the audited
hashes at `c69a09246eb6263665defd9d79c8bc3d88266685`; the same hashes were checked
again on this documentation branch. All preserved snapshot hashes match.

| Original path | Complete difference and evidence |
| --- | --- |
| `apps/decodex-gpui/src/chief_async_questions.rs` | A redundant recovery guard is removed below the existing early return. Three test DTOs add `arrived_live: false`. The cold-restore test proves that recovery retains saved answers without restoring inputs early. |
| `apps/decodex-gpui/src/chief_detail_wire_tests.rs` | The existing read-only detail test adds command and MCP tool kinds. Every original case and assertion remains. |
| `crates/decodex-codex/src/app_server_client/app_tool_exposure.rs` | Only two accessors are added. The runtime uses the writable file identity and reviewed version for shared configuration arbitration, delivered by PR1481. The inherited implementation is otherwise identical. |
| `crates/decodex-runtime/src/chief/tests/external_context.rs` | Wake and delegation labels become `automation` and `goal`; the obsolete root `composer` assertion is removed. Tool output, empty user input, exact history and no-replay assertions remain. Current dispatch assigns direct root input the `user` label. Four local context tests pass; two native opt-in tests were not run for this audit. |

At fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`app-server-protocol/src/protocol/v2/turn.rs` defines `turn_trigger` as an optional
string source classification. It is ignored when steering an active turn.
`core/src/session/turn_input_tests.rs` verifies that an automation-labelled steer
does not replace the active trigger. The installed 0.158.0-alpha.2 schema confirms
this wire shape. Local tool authority remains in `toolOutput`; it does not depend
on the spelling of the source label.

These four decisions reduce pending content-review rows from 223 to 219 after
the prior four-row batch. The register retains all 360 rows. This does not close
shared production files, skipped native tests, or final desktop acceptance.

## Delivered recovery files reconciled on 2026-09-26

Three rows now reference merged recovery deliveries. The full comparisons were
made at `d337ec4d3da23259e23390e304dbe33e0272322c` and the same hashes were verified
again at `3790eb3569301b151323acfbfc890bd5d3da4ee6`.

| Original path | Delivered relationship |
| --- | --- |
| `crates/decodex-codex/src/app_server_client/history_summary.rs` | PR1528 restores the complete inherited implementation and negative tests. The only additional content is a positive read-only, chronology and cursor-removal test. |
| `crates/decodex-runtime/src/chief/timeline/summary.rs` | Exact preserved file bytes after PR1528. The service, client, GUI and installed-native qualification are recorded in `history-summary-recovery.md`. |
| `crates/decodex-runtime/src/chief/result_messages.rs` | Exact preserved file bytes after PR1531. Native timestamp bounds and paginated durable recovery are recorded in `native-terminal-times.md`. |

All three original snapshot hashes match. All 360 rows remain; pending
content-review rows decrease from 219 to 216. This closes only these three files,
not shared timeline consumers or final signed desktop acceptance.

## Model source foundation and warning fixture reconciled on 2026-09-26

Four complete file comparisons were checked against merged main
`3dc4fab3df1353192b9cfa452db8d8fdcb43c39b`. All original snapshot hashes match.

| Original path | Delivered relationship |
| --- | --- |
| `database/migrations/0035_initial_model_source.sql` | Exact bytes registered as migration 46 by PR1537. The upgrade fixture preserves old requests and nullable settings, rejects invalid source pairs and flags, and retains the new state on repeat migration. |
| `database/src/conversation_routing.rs` | Exact preserved file bytes after PR1537. Account/revision mismatch tests verify that rejection creates no route, session or admitted turn. A later matching account still requires review after reopen. |
| `database/src/conversations/initial_model_source.rs` | PR1537 retains the complete receipt, revision, authority and input-preservation logic. The only changes adapt reasoning effort and service tier to current nullable types, their validation, digest encoding and SQL binding. Concurrent confirmation, stale revision, cold replay and post-route refusal passed. |
| `crates/decodex-runtime/src/account_launch/chief_process_warning_tests.rs` | The complete difference adds an explicit `ServerEvent` import and adapts the provider helper to `serve_fixture`, with the same usage/output and no body capture. All warning transport, owner, history and reopened-store assertions remain. The installed-native test passed with Codex 0.158.0-alpha.2; current file bytes match that tested file. |

The database foundation passed all 147 library tests, seven restart integration
tests, and strict database/runtime lint. The source-bound full continuation
fixture and public creation-receipt restart test also passed. The warning test
passed independently with an isolated local provider.

All 360 rows remain. Pending content-review rows decrease from 216 to 212.
These dispositions do not close the pending PR1538 protocol/desktop integration,
shared production files, R03 acceptance or final signed desktop acceptance.

## Restored model review files reconciled after PR1538

Five complete comparisons were checked at merged main
`1f35c8d695e8e912c160da6612e59bf0ea34a11b`. All original snapshot hashes match.

| Original path | Complete difference and evidence |
| --- | --- |
| `crates/decodex-protocol/src/model_catalog.rs` | All inherited DTOs remain. The current optional defaults omit serialization when absent, retain default decoding, and add a backward-compatible round-trip test. The 161-test protocol suite passed. |
| `crates/decodex-runtime/src/application_model_review_confirmation_tests.rs` | Exact restored file bytes. Installed-native confirmation submits once across retries and verifies cold readback. |
| `crates/decodex-runtime/src/application_model_review_native_tests.rs` | All inherited tests remain. The fixture adapts effort to `Option` and adds an assertion that runtime recovery returns the explicit review-required state. All three installed-native tests passed. |
| `crates/decodex-runtime/src/application_account_nudge_native_tests.rs` | Registration is restored. The only remaining full-file difference clarifies the isolated-home error text. The registered model-review fixtures ran successfully. |
| `crates/decodex-runtime/src/routing_orchestration.rs` | The only full-file difference removes one private-variant comment. All routing behavior, including review-required refusal, matches the preserved source; installed-native recovery exercises it. |

All 360 rows remain; pending content-review rows decrease from 212 to 207.
File preservation does not establish an active desktop consumer. The separate
signed desktop inspection did not find a current History entry and did not pass
interactive ordinary-model confirmation. Its optional-workbench classification
and failed acceptance remain explicit in `initial-model-source-recovery.md`.
Shared application, shell and conversation files are not closed by these narrow
file comparisons.

## Three delivered owners reconciled on 2026-09-26

Full-file comparisons and snapshot hashes were checked at
`1f35c8d695e8e912c160da6612e59bf0ea34a11b` and rechecked after PR1540 at
`5eb65fdb56a14309e09fd67a27a66901df7366e0`.

| Original path | Complete difference and evidence |
| --- | --- |
| `apps/decodex-gpui/src/chief_detail.rs` | Model invalidation moved from detail-panel closing to the stale-service and unsuccessful-snapshot owners in PR1530. The only other difference is the order of two entries in a membership list. The regression proves invalidation on service loss and retention when only details close; see `model-observation-recovery.md`. |
| `crates/decodex-runtime/src/chief_install.rs` | Only comment wording differs after PR1533. Executable source is identical, including native request liveness after local turn completion. See `install-request-liveness.md`. |
| `tests/scripts/test_vnext_architecture.py` | The expected protocol version changes from 2.50 to 2.90. The staging assertion uses the Cargo metadata target directory and adds two assertions for that owner. All other test content is identical. The 15 architecture checks and the signed stage build passed for the delivered source flow; see `initial-model-source-recovery.md`. |

These decisions reduce pending content-review rows from 207 to 204. All 360
register entries remain. This documentation batch changes no production code and
does not close shared desktop acceptance or any other incomplete file.

## Review and model-observation coverage restored on 2026-09-26

Two inherited test blocks had been removed while their production behavior
remained. Restore the missing scenarios and compare both complete files.

| Original path | Complete disposition |
| --- | --- |
| `apps/decodex-gpui/src/chief_misalignment.rs` | Exact snapshot bytes restored. After findings are reviewed, a missing continuation request must keep the action hidden and cannot submit. The existing stale-review and second-click checks remain. The focused rendered test passes. |
| `database/src/chief/tests/turn_execution.rs` | Restore the model-observation write before the one-item visible-history read. Use the current `record_chief_task_models` owner and require a positive record ID. The internal record must not consume the visible page or wake work. All original remaining assertions are retained; the focused persistence test passes. |

Both original snapshot hashes were verified. These changes restore test coverage,
not a newly reproduced production failure. The complete register retains 360
entries and has 202 pending content-review rows after this batch, down from 204.
This does not close any shared desktop acceptance group.

## Conversation history invalidation restored on 2026-09-26

`apps/decodex-gpui/src/client_lifecycle.rs` again reloads an open history page
after `ConversationChanged`, which the service publishes after explicit recovery.
It also retains `ConversationTurnFinished` and the added
`ConversationHistoryChanged` branch. Full-file comparison shows no other
differences from the preserved snapshot. The original hash was verified and the
current hash is recorded. This closes one content-review row; see
[history refresh recovery](history-refresh-recovery.md) for the reproduced
failure and event-routing boundary.
