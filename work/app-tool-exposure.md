# Connector tool exposure

This is an optional product capability in the fixed upstream update. Native Codex
owns tool filtering. Decodex must not construct another tool availability engine.

## Native contract and adapter

Upstream commit `a6d4741d3968ee0b9984e896db08d6fc4aef05e8` adds
`apps.<connector_id>.omit_tools_from`. The fixed cutoff is
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. At that cutoff,
`codex-rs/core/src/tools/spec_plan.rs` combines connector and server omissions.
The installed `codex-cli 0.155.0-alpha.16.4` ConfigReadResponse schema exposes
`code_mode`, `deferred`, and `direct`.

The adapter reads effective and writable connector preferences through native
`config/read`. An absent value inherits configuration; an empty list explicitly
clears the connector preference. A clear does not override server restrictions.
Unknown future read values remain visible; writes accept only supported values.

An edit changes one quoted connector leaf through versioned `config/batchWrite`.
The connection and history guard must match the reviewed source. The adapter
reads back the saved value and preserves higher-layer override feedback. It does
not retry a failed or uncertain write. The retained bridge rejects global defaults,
account-level paths and additional edits. App approval settings are unchanged.

## Evidence and remaining product work

The installed-native fixture verifies cross-client conflicts, empty versus
inherited settings, cold restart, and preservation of connected-account approval
settings. A second native fixture verifies that invalid configuration is not
silently overwritten and its parse cause remains available without Debug leakage.
Both use disposable configuration directories and start no model turn.

The complete adapter suite passes 185 tests with 7 opt-in tests skipped. The two
new native tests pass explicitly. The retained bridge suite passes 19 tests.
Strict adapter and runtime Clippy results are recorded in the batch ledger.

This adapter is not a delivered desktop control. The service, durable receipt and
UI integration remain pending. The inherited task-scoped receipt implementation
predates the current shared hook/app configuration journal. Adapt it to that
canonical writable-file owner before enabling edits. Preserve unresolved writes
across clients and restart. Do not copy the old shared files over current owners.
