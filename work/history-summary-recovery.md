# History summary recovery

## Contract

If the initial full native history read fails, Decodex can display recent prompts
and final replies from the native summary view. This view is incomplete. The UI
states that intermediate messages and tool activity are unavailable.

The reference is `openai/codex` commit
`595cc91e8cbb1c2ca822d0311dcf12709410c582`, in
`codex-rs/tui/src/app_server_session/rollout_history.rs`.
Installed Codex 0.158.0-alpha.2 generated a schema that includes the summary
items view and paginated history mode. Its isolated public-socket fixture passed
a real summary read with user and assistant content. The parent latest turn and
local model-provider request count stayed unchanged. This proves native summary
compatibility, not a real-provider outage or signed desktop failure recovery.

- Read metadata first and require the exact thread with paginated history.
- Request descending summary turns, then display them in chronological order.
- Start with 100 turns. Reduce the requested count if the public projection
  exceeds its 60 KiB response budget.
- Do not resume the thread, acquire a writer lease, or send conversation input.
- Do not recover an older-page request with a summary.
- Recheck the work source before and after recovery. Reject a changed account,
  process generation, thread, work revision, or history revision.
- Keep summary items separate from native timeline entries. Do not invent native
  positions or retain a full-history cursor or voice boundary.
- Permit original-text copy and existing source-bound attachment previews.
- Replace summary state when full history becomes available.
- Keep prompt handback and automatic recap dependent on full history.

Protocol 2.89 adds the `Summary` result. The client rejects a result for another
work item or thread. The total client timeout is 45 seconds, which includes the
25-second full-read budget and 15-second recovery budget.

## Current validation

Adapter tests verify exact read-only requests, chronology, metadata and page
validation, and cursor removal. A service fixture verifies successful recovery,
failed recovery, older-page exclusion, and source changes. Two GPUI tests verify
the omission notice, original markdown copy, summary reset, and removal of old
positions and cursors. A client socket fixture verifies valid and crossed work
or thread identities. The complete protocol suite passed after its exact-version
goldens were updated.

The initial strict lint run found existing long functions, unwrap calls, and
capture-only helper warnings in the protocol and GUI packages. The affected
protocol, runtime, adapter, and GUI packages now pass all-feature, all-target
strict lint. Five capture-only methods have a local dead-code allowance because
the shared module is also compiled into the main binary. Their workbench callers
remain present and compile under the same gate.

The full GUI suite passed 503 tests, with five existing opt-in tests skipped.
The service timeline regression passed 37 tests. The installed native fixture also passed after the summary check was added.
Final current-artifact acceptance remains open; these test results do not prove
installed desktop failure recovery.
