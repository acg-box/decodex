# Preserve ordinary model boundaries

The inherited ordinary model selector excluded `gpt-reserve` from its catalog,
transport payload and reservation. The current manual selector lost those checks
when it moved to a separate model-only request type. Restore them in the existing
owners. A configured native model remains readable; this change only prevents an
ordinary model control from entering Reserve.

Fixed upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582` distinguishes Reserve from
ordinary usage in `protocol/src/error.rs`. Its TUI
`luna_reserve_recovery_tests.rs` uses a separate recovery flow and a catalog entry
with `show_in_picker=false`. This batch preserves Decodex's inherited ordinary
scope; it does not implement that separate Reserve recovery workflow.

The inherited reconciliation also required an enabled current account. Move the
existing legacy-account availability check to the shared model observation
entrypoint. Current and preserved journals now use that one check. A disabled
account cannot release an old unknown model request, even after the old process
is confirmed dead. A later valid publication can reconcile the request without
resending it.

Before the fixes, both the adapter and database accept a Reserve selection, and a
disabled replacement owner marks an unknown request superseded. The regressions
fail at those assertions. After the fixes, 19 adapter model tests, 19 database
model tests, two service fixtures and strict adapter/database/runtime Clippy pass.
The service catalog deliberately advertises Reserve to verify that the ordinary
selector excludes it. Failure and validation logs:

- `/tmp/decodex-ordinary-reserve-before.log`
- `/tmp/decodex-model-disabled-owner-before.log`
- `/tmp/decodex-model-owner-boundaries.log`
- `/tmp/decodex-model-owner-service.log`
- `/tmp/decodex-model-owner-boundaries-clippy.log`

These are correctness boundaries for retained model controls and saved requests.
The controls themselves remain optional for the user's removal review. No new
journal, native workflow, schema or maintenance automation is added.
