> Historical record, archived on 2026-09-27. Statements and test results apply to the revisions named below. Use the [OpenWiki quickstart](../../../openwiki/quickstart.md) for current behavior.

# Current Code Mode classification

Code Mode execution, cells, timers, tool-call metadata and compaction remain
native Codex responsibilities. Decodex retains source-bound activity and history
presentation. It does not add a JavaScript engine, tool filter or compactor.

Current fixture ownership and installed-native evidence are recorded in
[native Code Mode qualification](native-code-mode-qualification.md). Fresh
qualification and the separate optional App UI scope are recorded in
[native tools reconciliation](native-tools-record-reconciliation.md).
Cross-turn Guardian policy, still-running nested calls during compaction, failed
metadata rebudgeting and signed desktop acceptance retain their explicit limits.

## Preserved historical record

The original note follows unchanged. Its probe versions, source-review positions,
counts and remaining-work wording describe the preserved branch. They do not
establish current acceptance or require a local replacement for native policy.

# Native Code Mode integration

Review cutoff: `595cc91e8cbb1c2ca822d0311dcf12709410c582`.

Codex owns execution delegates, originating step settings, nested tool dispatch,
notification token budgets, and helper lifecycle. Decodex uses app-server events
and native history. It does not implement the Code Mode session protocol or
rebuild model context from raw response events.

Upstream changes `3305c4f31d4ab6542afa05b91e112f1d67e9744f`,
`2fc4bda3ca3e93b764dd3845cb8c1a15a10866e1`, and
`122d55cba84993fb21af3f9cc342e099b8f690a4` retain each cell's delegate,
originating tools and output budget after a yield. MCP completion events and
legacy history carry the originating turn. Guardian denial accounting uses the
currently serviced turn; its cutoff implementation lives in the native extension.

## Local change

Chief previously discarded MCP activity after its originating turn completed.
The activity store now accepts it when the exact native thread and terminal turn
receipt are present and the payload identities match. It retains the old turn,
does not wake work, and does not change the current dispatch. Duplicate events
remain idempotent. Native history remains authoritative for complete tool output.

## Evidence

- Installed native Code Mode and its packaged helper execute a yielded cell,
  another cell, and a wait. Outputs and notifications retain their exact call IDs.
- An isolated MCP server holds a call until a second turn starts. Its completed
  event retains the first turn ID. Native history before and after a process
  restart contains the item once in the first turn, with no extra model request.
- The database regression first reproduced the discarded activity. The fix
  preserves it across database reopen, rejects wrong thread and payload identities,
  keeps the new turn running, and adds no wake event.

The tests use synthetic local model responses and no real credentials. See
`chief_process_native_code_mode_tests.rs` and `chief/tests/activity.rs`.

These checks do not qualify signed desktop interaction, the attested production
helper launch path, cross-turn Guardian interruption, or model-switch notification
budgets. Keep those acceptance items open. No local replacement for native policy
or truncation is required by these source changes.

## Direct invocation metadata

Upstream `1715e55076737158ba61d43158ede504de6d4ce1` binds each direct-call
record to its own invocation output, including reused call IDs. Completion means
that the call inventory and arguments were recorded; it does not mean the tool
succeeded. Pending reservations release on completion or cancellation. Disabling
capture invalidates prepared records and prevents their replay. Native code owns
the per-call, request and retained-history metadata budgets.

At cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, compaction also attaches
Code Mode observations with a separate budget while preserving captured direct
records. Raw app-server response notifications still remove executed-call
metadata. Decodex opts out of those raw notifications and uses typed events and
native history; it does not reconstruct completeness from private metadata.

Installed 0.155.0-alpha.16 qualification passes with capture off and on using
an isolated local Responses endpoint and a missing-image tool call. An
OpenAI-named fixture provider retains the direct record in model input when
enabled. Raw notifications omit the record and preserve the exact tool output.
A third-party-named provider strips passthrough metadata for compatibility;
the initial probe incorrectly expected it there and was corrected after source
inspection. No live service or real credentials were used. Evidence:
`/tmp/decodex-1239-native.py` and `/tmp/decodex-1239-native-openai.log`.

The existing Decodex native context/restart test also passes in
`/tmp/decodex-1239-native-bridge.log`: typed history survives restart, raw
notifications are absent, and model requests are not replayed. These checks do
not cover native reused-ID concurrency, budget exhaustion, compaction or signed
desktop acceptance; the corresponding upstream tests were inspected, not run.


## Timer cancellation qualification

Upstream `d77ebc72237a639b6d877f2edc3b20b54631f25e` cancels Tokio timer tasks
when cleared or when the isolate ends. Decodex delegates this to native Code Mode.
The installed-helper regression now clears a timer in a yielded cell and leaves
an unawaited timer in another cell that completes. After waiting for the first
cell, neither cancelled nor late callback output appears in model tool history.
The existing call-origin and notification checks still pass with four requests.

Evidence: `/tmp/decodex-1248-native.log` (one native test) and
`/tmp/decodex-1248-lint.log` (strict all-targets runtime lint). This checks output
lifecycle through the retained bridge, not native Tokio task counts or the
attested production helper launch path.


## MCP origin metadata

Upstream `d76109773497850ed2699f2816e3f7b1867d0c14` retains `sessionId`,
`threadId`, originating `itemId`, and `windowId` for nested MCP calls. The window
is the model context window, not a desktop window. A yielded cell retains its
origin instead of taking the identity of a later wait or turn.

The installed native delayed-MCP test now records metadata for two nested calls:
one blocks until a second turn starts, and the other runs after release in the
same cell. Both match the first model request's session/window and original cell
item. Each completed item occurs once under the original turn before and after
cold restart, with four model requests and no replay. The fixture records only
synthetic local metadata inside its temporary home.

This does not qualify compaction between nested calls or a missing originating
item ID. The upstream compaction regression was read; it was not run locally.


## V8 array-sort workaround review

Upstream `aaa2cabfbcb8d9997ce67e166f796f46d5b72342` disables Maglev, Turbolev
and TurboFan array builtin inlining during native V8 initialization. The final
source is unchanged. Its isolated regression requests optimization, mutates array
elements in a comparator and checks element-kind integrity plus ordinary sorting.
The installed ChatGPT Code Mode helper contains the exact initialization flag
literal. That binary observation is not a runtime optimization-state assertion,
and the optimization regression has not been run locally. Decodex delegates Code
Mode to the packaged native helper; do not introduce a separate V8 runtime.


## Guardian wrapper policy review

Upstream `8f38d5a877da8c5c2c0b5158e72bb5f2290a3157` removes the separate
`code_mode` approval category. With model-owned policies, a direct custom `exec`
wrapper leaves the nested tool's cached score intact. A dynamic function named
`exec` or an ordinary MCP tool named `wait` does not receive this exception.
Nested tools retain their own categories and required reviews. Legacy all-tool
configuration can still score wrappers.

The cutoff implementation retains the exact direct/custom/default-namespace
check. Later policy centralization adds `other_tools`, `unscored_action`,
`initial_cua_call`, and `sandboxed_exec_commands`; defaults and required model
policy still belong to Codex. Decodex has no local Guardian category or score
cache to migrate.

The upstream model/required-model/legacy test matrix and final relevant source
delta were read. The existing local yielded-cell test does not qualify Guardian
score reuse across cells. Native qualification must check that cached nested CUA
scores survive wrappers while ordinary same-named tools invalidate them, and
that synchronous and administrator-required reviews still occur. This acceptance
item remains open; no test was run for it in this review.

## Compaction metadata qualification

Upstream `20f4d12f76fc0dbe77bb4052e8acc1615bef5f5d` attaches pending and retained
Code Mode observations to local and remote-v2 compaction. Its separate metadata
budget preserves direct-call history and keeps observations absent from shortened
failed retries until ordinary sampling advances the live window. Final metadata
owner and remote-v2 call site are unchanged at the fixed cutoff. Later local
compaction changes separately protect post-turn history until success.

The installed-native `installed_native_code_mode_metadata_reaches_compaction`
test passes: one real packaged-helper MCP invocation supplies complete nested
metadata to the subsequent model request and the compaction request. The next
request and a cold-resumed request retain the exact compaction checkpoint. It uses
a temporary home, an isolated local Responses endpoint and synthetic MCP server.
No real credentials or external inference are used. Evidence:
`/tmp/decodex-1418-native-code-compaction.log`; full runtime repository Clippy:
`/tmp/decodex-1418-lint.log`. Failed/rebudgeted retries, compaction between two
still-running nested calls and signed desktop behavior remain separate gaps.

## Shared tool preview budget (1432)

Upstream `6749535c` initially keeps head/tail rows across Code Mode result blocks.
At the fixed cutoff, shared `ToolOutputPreview` replaces that policy with three
leading screen rows and explicit hidden logical-line counts. It bounds work on
large or combining-character lines before wrapping; expanded text remains full.
Do not copy the superseded tail layout as the final upstream requirement.

Local MCP/dynamic activity rows already omit raw output until the source-bound
native detail reader is opened. Standalone `functionCallOutput` did not have that
entrypoint and lost access to text beyond its 8 KiB preview. It now projects a
Tool result activity and opens the same paged detail owner. The shared output
extractor preserves text-block order, excludes image/audio/encrypted payloads and
uses the existing credential filter. No second full-output store is introduced.

A large Unicode multi-block result reconstructs exactly across pages, including
its final error; an unrelated thread is rejected. Five runtime tests pass in
`/tmp/decodex-1432-tool-details-final.log`. The rendered socket pagination test
passes for both file changes and standalone tool output in
`/tmp/decodex-1432-detail-ui.log`. Full runtime and GPUI Clippy pass in the matching
`/tmp/decodex-1432-decodex-*-lint.log` files. Signed desktop and installed-native
standalone-output acceptance are separate from these tests.

## Complete empty inventories (1435)

Upstream `8452164c` records a completed empty inventory explicitly as
`executed_tool_calls: []` with `tool_calls_complete: true`. Completion concerns
lossless tool-call recording, not successful execution: a cell can throw an error
and still have a complete empty inventory. Missing metadata is not equivalent to
an empty list. Retries revalidate history; late calls, duplicate outputs and
untrusted wait origins cannot acquire completeness. Final owners are unchanged.

The installed-native compaction test now covers a real MCP invocation, successful
ALL_TOOLS discovery without invocation and a thrown error before any tool call.
It verifies the expected output, exact inventory in both next-model and compaction
requests, and cold continuation from the saved checkpoint. Three cases pass in
`/tmp/decodex-1435-code-inventory-final.log`; full runtime Clippy passes in
`/tmp/decodex-1435-runtime-lint-final.log`. Native retry/termination/feature-off
cases were read upstream but not added to this installed test. No local metadata
consumer, UI loading inference or second recorder is introduced.
