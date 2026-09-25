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

## Service and desktop ownership

The installed App inventory uses native `app/installed` for the selected thread.
It preserves disabled and non-callable states and distinguishes unavailable,
unsupported, empty and over-capacity results. Reads do not force a live refresh.

The Tools and plugins panel opens a connector-specific editor. The first edit
from inheritance copies effective omissions before changing one surface. The user
can restore inheritance or clear connector omissions explicitly. Draft changes
require Save. Source changes discard the reviewed state; an uncertain command
reply triggers a read instead of another write. A saved setting is not proof
that an active model step changed its tool list.

Connector exposure reuses the existing App configuration receipt and shared native
file arbitration with hooks and connection approvals. Its target field is
`omit_tools_from`, with no connection link or pending approval request. This does
not create a new journal or database migration. Raw values preserve absent versus
empty settings. Another live writer cannot settle an uncertain attempt; the
existing process-death and exact readback rules remain in force. The shared
recovery owner reads the original connector target even from another settings
panel. Local protocol version 2.75 includes the query, command and inventory.

## Validation scope

The installed-native adapter fixtures verify cross-client conflicts, empty versus
inherited settings, cold restart, approval preservation and parse failures. They
use disposable configuration and start no model turn.

Database tests verify shared Hook/App/exposure exclusion and restart recovery.
The owned-process fixture verifies inventory identity, stale review rejection,
save, durable uncertainty and no replay. Rendered panel and local socket tests
verify inherited editing, source changes and lost-reply readback. Discovery tests
verify invalidation when the task directory or native settings change.

These fixtures do not prove a real installed connector's tools changed during a
live model turn. Signed desktop and live connector acceptance remain separate.

Batch validation passed: protocol134, adapter186, database117, runtime561 and
GPUI460 tests; 52 opt-in tests skipped. Strict all-target/all-feature Clippy passed
for all five affected packages.
