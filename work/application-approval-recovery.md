# Application approval recovery

## Problem and change

The inherited application projection allowed `connector_id`, `link_id`, and
`link_is_implicit` in native MCP request metadata. The current projection removed
these fields. The existing desktop form still uses `connector_id` and `link_id`
to show account settings, and uses `link_id` to label the connected account.
Restore the three inherited fields. Keep the metadata allowlist and the existing
private-field exclusion.

Source owners are `chief_request_metadata` in
`crates/decodex-runtime/src/application.rs` and `mcp_form_panel` and
`mcp_account_label` in `apps/decodex-gpui/src/chief_mcp_forms.rs`.
The fixed upstream commit `595cc91e8cbb1c2ca822d0311dcf12709410c582`
retains native account-link metadata in `codex-rs/core/src/mcp_tool_call.rs`.
Its `codex-rs/core/tests/suite/mcp_turn_metadata.rs` asserts the link ID and
implicit-link flag on native elicitation requests. This source comparison does
not prove support in every installed executable.

## Restored inherited coverage

- A live background approval remains visible after its original turn ends.
  A foreign thread and a request without live evidence remain unavailable.
- File approval pages retain the complete enriched diff. A changed diff or the
  original unenriched request cannot continue a page sequence with the old digest.
- A saved child file approval retains its full diff after the store reopens,
  without an active parent turn. A wrong parent or resolved request is rejected.
  This fixture calls the scoped projection with live evidence; the public service
  still requires its live host. It does not prove live-host acceptance.
- Command and permission approvals retain the native execution environment and
  working directory. A malformed environment is rejected, and private fields
  remain excluded.
- A task that requires model review before its native session starts exposes the
  model-review state and recovery action.

The inherited snapshot is `recovered-96ad-20260925`. The scoped child fixture
uses the current projection boundary; its ownership, restart, complete-diff and
resolved-request assertions remain intact.

## Validation and limits

Before the field restoration, the native elicitation projection test failed:
`connector_id` was null instead of `calendar`. The failure is recorded in
`/tmp/decodex-application-account-before.log`.

The combined branch, including the output-filter recovery, passes 53 application
tests; four existing opt-in tests were ignored.
Strict runtime Clippy passed with all features and targets. Logs are
`/tmp/decodex-application-coverage-combined.log` and
`/tmp/decodex-application-coverage-combined-clippy.log`.

This batch restores
existing behavior; it does not introduce a new optional integration. The metadata
repair is core compatibility for the retained desktop consumer. Account
settings remain an optional product surface for the later removal review, while
correct account identity is required if that surface is retained.

The full shared `application.rs` reconciliation remains open. In particular,
the oversized file-detail policy and the remaining query-owner mapping require
an explicit disposition. Local projection tests do not replace installed-native
or signed desktop acceptance. Automations remain paused.
