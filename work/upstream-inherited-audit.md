# Inherited change reconciliation

Audit baseline: Decodex `7410a97b690f95a8567253f9833a9d47d436d1dd` (PR1507).
The preserved working-tree snapshot started from
`2ffa385c3b49efe6a4109de0fd7353fb64abd2c5`. It contains 357 files and three
deletions. All 357 file SHA-256 values still match the takeover manifest.

This audit covers inherited files, not the 1,569 upstream commits. File counts do
not measure feature completion. A merged capability can touch many files, and
one shared file can contain both delivered and outstanding behavior.

## File evidence

[The complete 360-row register](upstream-inherited-files.tsv) contains fresh hashes
from the committed audit baseline. It separates direct content comparison from
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
