> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Keep preserved model journals out of message pages

The current history queries exclude current model-selection records but omitted
the preserved `model_recovery`, `model_recovery_result`,
`model_recovery_observation` and `model_selection_reconciled` kinds. Those private
records could consume a bounded page before the renderer considered user messages.
The inherited queries excluded all four kinds.

Restore the exclusions in the existing work-history and transcript queries, before
ordering and pagination. Keep the saved rows and the dedicated model receipt
reader unchanged. No data is deleted or rewritten, and pending-operation checks
still read the full journal.

The regression first returns a private observation instead of the visible answer
on a one-record page. After the fix, page sizes one and twenty both return the
answer through both history APIs. The same assertions pass after database reopen.
A separate restart-reconciliation case checks the reconciliation record, and the
dedicated receipt reader still returns the original response and observation.

All 20 database model regressions and strict database Clippy pass. Logs:

- `/tmp/decodex-legacy-model-pages-before.log`
- `/tmp/decodex-legacy-model-pages-after.log`
- `/tmp/decodex-legacy-model-pages-clippy.log`

This restores message-history correctness. It does not add a product feature or
close the remaining whole-file review of the shared history owner.
