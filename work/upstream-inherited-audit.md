# Inherited change reconciliation

Audit baseline: Decodex `3f131d80b9e90d2badf2394249bbf3b0266f72d3`.
The preserved working-tree snapshot started from
`2ffa385c3b49efe6a4109de0fd7353fb64abd2c5`. It contains 357 files and three
deletions. All 357 file SHA-256 values still match the takeover manifest.

This audit covers inherited files, not the 1,569 upstream commits. File counts do
not measure feature completion. A merged capability can touch many files, and
one shared file can contain both delivered and outstanding behavior.

## File evidence

[The complete 360-row register](upstream-inherited-files.tsv) records each snapshot
path, current path comparison, reconciliation result, known partial deliveries and
both content hashes. A partial-delivery PR does not close its whole source file.

| Verified disposition | Entries | Meaning |
| --- | ---: | --- |
| Exact content | 76 | Snapshot bytes equal the current committed file. This includes 53 newly reconciled entries. |
| Matching deletion | 3 | The snapshot records deletion and the current path is absent. |
| Exact registered migration under another number | 1 | Snapshot `0037_chief_async_skips.sql` equals current `0034_chief_async_skips.sql`, which the migration owner registers. |
| Replaced by the shared app configuration journal | 2 | The separate exposure journal/test are superseded by `chief_app_settings.rs` and its current tests (PR1481). |
| Requires content review | 278 | The file has remaining unclassified differences or no confirmed current owner. |

Raw path comparison: 76 exact files, 173 different files, 108 absent file paths
and three matching deletions. Of the 108 absent paths, three have the verified
replacement dispositions above. Absence does not by itself prove a feature gap.
Tests, historical work notes, renamed modules and proposed migrations also appear
in the snapshot.

The original PR1378 is still open. These stash objects remain present:

- `9187391b7ffc569f8d304ae3bfdfea5c32d566cc`
- `3a9454d5457882473cb49697872ea6403ecf4b29`

Do not close the original PR or remove the snapshot/stashes from these counts.
They prove preservation, not complete integration.

## Concrete remaining core gaps

| Gap | Current evidence | Required next work |
| --- | --- | --- |
| Native pre-dispatch refusal | `chief.rs::unsent_request_refusal` only selects local stale-history/size/queue refusals. `ChiefDispatchRefusal` has unused native draining/provider-change variants. | Inspect the fixed native error contracts and connect exact positive refusal evidence to existing unsent-input handling. Keep unknown outcomes fenced. |
| Claimed capacity retry refusal | Migration16 allows pending-to-cancelled and claimed-to-submitted, but not claimed-to-cancelled. `reject_chief_dispatch` already attempts the latter with a receipt. | Reconcile inherited migrations40/42 through one current forward migration and exact refusal tests; do not renumber old applied files. |
| Large native approval payloads | Current `insert_chief_event` rejects payloads over 65,536 bytes. The inherited `chief_request_payloads` owner and migration41 are absent. | Review full source-bound payload storage, selected detail pages, native liveness and one-shot replies. Do not treat a larger JSON limit alone as completion. |

These findings have current consumer evidence. The audit itself does not implement
them or promote historical test logs into current acceptance.

## Optional and separately reviewed work

The preserved notes describe voice preferences, public reasoning summaries,
model-access metadata, account analytics, MCP App widgets, earlier-prompt editing
and task recaps. These are separate product or presentation scopes; scanning their
upstream commits did not deliver them. Their inherited owners and current
alternatives still require reconciliation. In particular, existing quota windows
are not the full analytics report contract, and existing native revert observation
is not an edit-earlier-prompt action.

For this manual fixed-cutoff pass, the user authorized completion followed by a
product subtraction review. Future optional additions require user selection.
The scheduled automation remains paused, including after manual completion.

## Validation boundary

This documentation audit checked snapshot hashes, current committed bytes, the
registered migration, current shared-journal consumer, targeted refusal and
payload owners, stash identities and GitHub PR state. No application code or
production data changed. The register deliberately leaves uncertain rows open.
