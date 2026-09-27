# Native task model publication qualification

Restore the inherited task model fixture through the current `chief_models`
service and durable model-selection journal. Retain the original normal and Plan
mode scenarios. Use the existing native process, ownership and Responses fixtures.
No production selection policy or database migration changes.

The installed Codex CLI 0.158.0-alpha.2 passes both scenarios:

- Start a turn with `gpt-5.6-sol` and low effort, then pause on a native tool call.
- Read the current service catalog and submit a reviewed task-default change to
  `gpt-5.6-terra` with high effort. The receipt is queued; the paused tool remains
  paused and no model request is added.
- Observe the exact native settings publication, preserve the service tier and
  Plan mode, persist through the current owner, and reopen the store. The receipt
  is `target_observed` and retains its process-generation identity.
- Release the tool. The current turn still uses its original model and effort.
  After native process restart, the following turn uses the saved new selection.
- Create a separate task. It uses its independent original defaults. Exactly four
  model requests occur; their wire model and turn metadata match the expected
  sequence. Plan instructions remain on the original task and do not leak into
  the new task. The configuration file is unchanged.

The restored test and strict runtime Clippy pass with all features and targets.
The fixed upstream `thread_settings_update` test verifies settings publication,
no inference from a settings-only update, and subsequent-turn behavior. Its
source was read at `595cc91e8cbb1c2ca822d0311dcf12709410c582`; upstream tests
were not executed here.

The complete inherited test-file disposition is closed. The retired recovery
journal API is mapped only to the current fixture's selection and publication
behavior. This does not prove compatibility with persisted legacy recovery rows
or automatic fallback policy; those remain open. Shared reviewer fixtures and
signed desktop acceptance remain open. No real account, installed configuration
or automation is changed.
