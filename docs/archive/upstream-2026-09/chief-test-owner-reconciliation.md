> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Reconcile the shared Chief test file

Compare the complete preserved `chief/tests.rs` diff at fixed upstream cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. Retain current source ownership and
restore assertions lost during extraction. This batch changes tests and records,
not production behavior.

## Restored coverage

Restore the original freeform asynchronous `final_answer` case. Two deliveries
produce one message, do not add a question, and do not complete or wake the task.
Restore all three MCP form mode spellings in schema validation and resolved or
reconnected request tests. Invalid responses must not consume a live request;
resolved and old-connection requests must not replay a reply.

Restore start policy, sandbox and working-directory assertions in the unload
test, and verify resume does not overwrite existing native settings. Restore the
full known/unknown, latest-success/failure and missing-original-turn matrix for
misalignment recovery, including database reopen. Keep the separate current
history-reconciliation test for missing items and voice retirement.

The preceding [output coverage repair](chief-output-recovery-coverage.md)
restores plan finality and exact-thread output invalidation after revert.

## Complete difference mapping

| Difference | Current disposition |
| --- | --- |
| Module registration | Large approvals and live file evidence move into registered dedicated modules. Async skip and missed-active-turn recovery move within the shared file. Authentication recovery remains registered. |
| Old task model-settings module | The registered `settings_observations` module retains the complete behavior through the current transport owner. See [all six inherited case mappings](native-settings-observation-tests.md). |
| Effort and model defaults | Keep explicit inheritance, provider-defined effort and legacy configuration tests. Turn requests inherit native model selection where no override is requested; do not restore an obsolete unconditional model field assertion. Current `native_settings` and capacity tests verify explicit selections and partial changes. |
| Fixture helpers | Extract thread-read and review-event helpers. Keep native item paging, settings updates and exact source identity. Honor `excludeTurns` and paginated history instead of supplying omitted history. The coordinator usage repair depends on that faithful fixture behavior. |
| Dispatch failure | Keep added database-reopen coverage for an uncertain thread start. A fresh connection cannot materialize the root again. |
| Sparse terminal history | Retain id-less historical agent messages as terminal evidence. The complete-output recovery tests separately require item IDs for final text replacement. |
| Output and questions | Keep output-revision notification, restored plan/revert cases, async skip durability, changed-content rebuild and partial-output source identity. Complete, missing and empty final readbacks remain distinct. |
| Resume and settings | The unload case retains original policy assertions. `native_settings` also compares the entire resume payload and verifies later partial model/effort changes. The existing capacity-retry refusal fixture checks same-thread no replay; the shared resume owner is not replaced. |
| Refusal classification | A stale settings guard for a claimed running dispatch returns `InputNotSent(SettingsChanged)`. A foreign guard before a dispatch claim remains `StaleHistory`. Preserve both tests and unchanged work state. |
| Native trigger | Explicit idle user answers use `turnTrigger=user`; steering keeps the existing turn. |
| Misalignment | Keep live-authority and stale-history checks. Restore the original full recovery matrix and retain the added exact-history reconciliation module. |
| MCP replies | Restore alias coverage, original schema validation and resolved/reconnected no-replay assertions. The installed child MCP limitation remains a separate qualification. |

These mappings do not qualify every ignored native test or signed desktop
behavior. Shared bridge and installed fixture registry reviews remain separate.
Core correctness includes source identity, durable evidence and no replay.
Optional controls remain subject to the user's removal review. Automations stay
paused, including after completion.

## Validation

All 159 Chief tests pass with no failures and 15 existing opt-in cases ignored
in 81.11 seconds. The log is `/tmp/decodex-chief-request-coverage.log`.
Strict stable runtime Clippy passes all features and targets in 18.17 seconds;
the log is `/tmp/decodex-chief-request-coverage-clippy.log`.
Verify the original snapshot hash and refresh the current owner hash. Close only
the shared Chief test-file row after this complete mapping and restored coverage.
