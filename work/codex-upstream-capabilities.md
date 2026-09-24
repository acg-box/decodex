# Codex capability reference

## Native task settings display — 2026-09-24

Protocol 2.51 adds a read-only task model query. The runtime checks the exact
work, thread, account revision, process generation, and history source before and
after the native read. A concurrent settings change invalidates the reply. Missing
metadata stays distinct from explicit null values. The query does not resume the
thread, send input, or change settings.

The desktop composer and model picker display the task model and reasoning effort
from this observation, unless the user has an explicit next-message choice. The
settings panel shows the native provider and permits an explicit refresh. The
composer and inspected task retain separate scoped observations. Connected
snapshot polling refreshes observations at most once every two seconds. Changing
profile, task, thread, turn, or native source clears the old observation; an old
async reply cannot replace a newer source or user choice. Unreported values do not
fall back to startup defaults. New model choices use the new model's capabilities.

Runtime tests cover source changes, unknown metadata, and foreign-thread refusal.
Rendered desktop tests use the public local socket to read known, null, missing,
and mismatched responses, and cover source changes and explicit-choice precedence.
These are read-only owner and rendered interaction checks, not signed-app release
acceptance. Broader permission/plugin observations remain outstanding.

## Native task settings adapter — 2026-09-24

At cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, `Thread.model` and
`Thread.reasoningEffort` expose loaded or persisted configuration through exact
`thread/read`. These fields are not per-turn inference telemetry. The adapter reads
them without resuming a thread or sending input, keeps native provider identity,
and distinguishes an older server's missing fields from explicit null values.
Start/resume replies and settings notifications have separate bounded projections;
private collaboration instructions are excluded.

A per-thread settings guard extends the existing connection/history write fence.
The transport invalidates it when `thread/settings/updated` arrives, before the
coordinator consumes its event queue. Unrelated task updates do not invalidate the
guard; disconnect and history replacement do. Guards retain only live readers,
with a bounded map. A duplex test proves that an already queued settings change
prevents the subsequent write.

The coordinator now restores existing threads without creation defaults. This
applies to recovery, ordinary dispatch, external-writer recovery, Guardian approval,
misalignment continuation, and voice recovery. Voice startup retains only its
explicit realtime feature opt-in. New tasks still receive creation defaults.

Before a normal turn, omitted model/effort fields inherit native task settings;
explicit partial user selections remain intact. The settings guard also retains
an async answer's question-state constraint. Requested selection is committed in
the same transaction as the exact native turn acknowledgment. It is not inference
telemetry and does not occupy visible transcript pages.

Capacity retries compare current settings with that saved acknowledgment, including
after reopening the store. Changed or unavailable selection cancels the old retry.
A matching native settings notification keeps the retry; changed settings retire
it without a new turn. A known pre-write guard refusal preserves unsent input for
user decision. A lost turn response, or failure after external-context injection,
remains uncertain and cannot authorize replay.

Validation: native cold restart preserves a custom effort and original model even
when the restarted coordinator has different defaults. The loopback backend sees
one initial request and one continuation; both exact selections have atomic journal
receipts. Adapter guard tests cover combined question/settings constraints, and
coordinator tests cover partial edits, changed-selection cancellation, store reopen,
and known-unsent versus lost-response handling. Task-settings presentation and the
broader permission/plugin observation migration remain separate outstanding work.

## Workspace policy for quota activation — 2026-09-24

Reference: openai/codex `0a5b9991698e8e3c126da6101aa9e4da421f7ddd`,
checked against cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`.
Native app-server resolves the selected workspace, required backend origin, and
workspace routing override. Its model provider applies that policy to Responses,
compaction, and WebSockets. Account usage, profile, reset, and analytics endpoints
remain account-backend scoped.

Quota activation now obtains its model destination from an attested, short-lived
native control process. It reads `account/read.workspaceRouting` and fresh
`configRequirements/read`, checks the exact selected account and required origin,
and applies both workspace routing and managed residency headers. The existing
Responses path, empty tool list, `store:false`, and durable no-replay fence remain.
No thread or model turn is created for policy discovery. Missing, unsupported,
malformed, changed, or unavailable policy prevents activation; account observation
and reset operations remain available without the optional native capability.

The API credential owner retains its per-account lock through discovery and HTTP
dispatch. The native child cannot rotate this credential or fall back to ambient
authentication. Refresh remains with Account Service before the lookup; a native
refresh request makes this lookup unavailable. A later observation can retry only
after the existing rejection backoff. The child is shut down before the model
request. No routing cache or shared account process is introduced.

Validation: the installed `codex-cli 0.155.0-alpha.16.3` fixture uses production
executable attestation and ephemeral synthetic credentials. It proves selected
workspace rather than default workspace, invalid-discovery refusal, fresh discovery
after restart, and no credential file. It sends no model request. Unit checks cover
independent residency and routing headers, changed requirements, incomplete policy,
unsafe origins, redirect refusal, and no retry after ambiguous HTTP failure.
This is native policy and local transport evidence, not a live quota activation or
signed desktop acceptance claim.

## Dynamic reasoning effort — 2026-09-24

Reference: openai/codex 595cc91e8cbb1c2ca822d0311dcf12709410c582,
`codex-rs/protocol/src/openai_models.rs`. Native ReasoningEffort retains custom
model-defined strings. Installed codex-cli 0.155.0-alpha.16.3 supports this path.

Protocol 2.50 retains bounded custom effort values across model discovery,
explicit message settings, desktop controls, and cold draft storage. Unknown
values display their exact name instead of High. Legacy `x_high` saved values
still map to native `xhigh`. Both protocol and native request adapters accept
up to 128 UTF-8 bytes and reject empty values and control characters.

Validation: 128 Codex adapter tests, 109 protocol tests, 485 runtime tests, and
310 desktop tests passed. Strict Clippy passed for all four affected packages.
An isolated installed-native test reads the model catalog through Decodex,
starts Chief with the advertised custom effort, and checks the exact outbound
Responses value at a loopback backend. It completed with one model request and
no real credentials. This proves the custom-effort path, not all model capability
or workspace-routing integration.

This batch is rebased on workspace PR #1393 (37a88d008). It retains the native
agent hierarchy, live output subscription, weather cards, interruption controls,
and the previously delivered draft and receipt recovery. Small owner extractions,
explicit imports, and public field documentation repair strict validation failures
introduced by that baseline; no lint checks are disabled.

## Earlier usage audit

The following reference and acceptance notes describe the earlier usage change.
They are not the current manual catch-up completion status.

Reference checkout: `target/upstream-codex` (ignored, read-only reference use).
Official repository: https://github.com/openai/codex
Reviewed main commit: fd346b8dbaa24573a0244bc917811849d27c4cf4
Installed binary: codex-cli 0.154.0-alpha.6.2

## Current findings

- `codex-rs/tui/src/token_usage.rs` distinguishes the latest active context from
  cumulative session consumption. Context uses `last.totalTokens`, with capacity
  from `modelContextWindow`. Decodex shows raw reported occupancy; it does not copy
  the TUI's presentation-specific reserved-baseline adjustment.
- `codex-rs/app-server/src/bespoke_event_handling.rs` forwards `TokenCount` snapshots
  as `thread/tokenUsage/updated`. A snapshot is not an incremental debit. Repeated
  notifications must not add the previous request's usage again.
- `codex-rs/app-server/src/request_processors/token_usage_replay.rs` replays the
  persisted usage snapshot when a client attaches to an existing thread. Decodex
  accepts one expected historical turn from the exact resume response, preserves
  newer live observations, and restores current context and a turn baseline when
  the installed provider emits that notification. It does not require replay to
  be present to continue a conversation.
- The installed binary's generated schema exposes `threadId`, `turnId`, cumulative
  `total`, latest `last`, and optional `modelContextWindow`. It does not expose a
  direct completed-turn usage field. Decodex therefore snapshots the cumulative
  counters at turn admission and seals their delta into the completed-turn record.
- A fresh provider thread starts with known zero consumption. Unknown historical
  baselines, counter resets, and completion recovered without final usage remain
  unreported. Resuming a thread changed by another client invalidates stale counts.

## Product result

Show work duration and input/output token counts once below each completed reply.
Show only current context occupancy near the composer. Preserve the reported
numbers, do not estimate them from text length or model names, and never use
cumulative token consumption as context occupancy.

Migration 20 adds nullable turn baselines to the existing service-owned usage
snapshot. Completed counts are retained in the exact immutable completion event.
Protocol 2.19 exposes optional turn usage. Earlier records remain compatible with
missing usage; no historical values are fabricated.

## Recurring review

The Codex Upstream Maintainer remains paused during the manual catch-up.
Its configured daily time is 20:05 UTC, or 04:05 Beijing time, without seasonal
changes. Resume only after the manual catch-up meets its completion requirements.

## Validation and preview

Database, protocol, GPUI, and runtime suites passed for the usage change; the final
runtime suite includes 302 passing unit tests plus integration tests. Strict Clippy,
repository formatting, architecture checks, and signed staging passed. Inspected
`target/visual-tests/chief-turn-usage.png`: per-turn metrics are below the reply and
only context remains below the composer. This is a native capture fixture, not a
fabricated result in the real Chief history. Reopened one signed preview process.
The existing external Codex writer conflict still prevents a live model round trip;
the request to release that exact Chief task remains unanswered.


## Integration observations across environment changes — 2026-09-24

Upstream `c775dd3c332de1b69b25a4580f6c5bc44b94e284` separates saved
thread environments from the environments of an active turn. The fixed cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582` retains this distinction.
Plugin discovery uses the configured repository; MCP status describes the loaded
native task. The desktop now states this distinction instead of presenting both
as the current execution repository.

Integration discovery captures the existing native thread-settings guard before
reading its scope. It rejects an observation if settings change during discovery,
even when the final directory string is unchanged. Changes to another thread do
not invalidate this observation. Native code remains responsible for environment
activation and MCP runtime ownership.

Upstream `0c9be8a836a65681bb4e2366f05590babf89edf2` preserves native
plugin, skill, and MCP caches after display-only metadata refreshes. Decodex status
discovery does not reload those caches. The separate explicit “Sync plugins and
reload MCP” action remains a deliberate reload. Native cache preservation was
source-reviewed, not qualified with the installed binary in this batch.

Validation covers stable discovery, directory changes, settings updates with the
same directory, unrelated-thread updates, and the desktop integration display.
No local protocol or database version changes are required.


## Native web-search details — 2026-09-24

Upstream `f8c6026c38682c628e71d1ae4978ba643499250a` preserves web actions
and structured results in exec JSON. Decodex uses the native app-server item
instead. At cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, its owner
is `codex-rs/ext/items/src/web_search.rs`: camelCase actions and optional opaque
JSON result arrays.

The detail view now retains search queries, page URLs, find patterns and result
objects, including unknown fields and error payloads. Missing results differ from
an observed empty array. Each result passes through the existing credential
filter before the existing 24 KiB UTF-8 display limit. Long details still show
that output was shortened; complete detail pagination remains separate work.
Activity labels distinguish search, page opening and page text lookup.

Tests cover projection, redaction, bounds and the native paginated-history RPC
path. These are synthetic transport tests, not installed-binary or signed-desktop
acceptance. No protocol or database migration is required.


## Complete activity detail paging — 2026-09-24

The activity detail query now returns an 8 KiB UTF-8 portion and a continuation
cursor. Its fingerprint binds the complete filtered text to the account revision,
process generation, history revision, work, thread, turn and item. The runtime
rechecks the source after reading. Changed content or ownership invalidates the
cursor; no pages from different observations are combined.

The desktop provides Read next portion and Back to start. It retains one portion
at a time, clears pending reads on navigation or disconnect, and rejects replies
from replaced sources or earlier open requests. A failed continuation remains
unavailable until the user reopens the detail. The client checks returned offsets,
page size and continuation consistency. File-approval previews retain their
separate existing bound. Web result projection from PR1403 is preserved.

Local protocol version is 2.52; the database stays at 34. Validation includes
complete Unicode reconstruction, changed-source and changed-content rejection,
invalid byte offsets, desktop stale-reply cases, and rendered continuation buttons
through the public socket. This does not certify signed desktop acceptance.


## Chief OpenAI form negotiation

At upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`,
`codex-mcp/src/client_capabilities.rs` projects the app-server client's
`openai/elicitation.form` extension to MCP servers. Decodex declares this extension
only for Chief connections, which retain an elicitation request consumer.
Ordinary conversation, probe, and activation connections do not declare it.

Unsupported schemas retain their original JSON through request projection. The
form UI offers decline and cancel but cannot accept unsupported constraints or
interpret an opaque OpenAI schema as an empty approval. Standard MCP null-schema
confirmations keep their existing behavior.

The isolated native fixture checks negotiation, exact choice values, duplicate
response rejection, unsupported-schema cancellation, and the native `never`
approval policy. Installed Codex `0.155.0-alpha.16.3` passed these cases with a
loopback inference fixture. This does not qualify MCP App UI, user verification,
standard-form-input extensions, or signed desktop interaction.


## Active context compaction

Upstream `a526f54b005f0647dec041f26a67356be88b15fe` and the cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582` keep compaction visible until its matching
item completes or the turn ends. Decodex derives the status from the selected
work's active turn and exact item identity. Background tool activity and a pending
send do not hide it. A completion from another turn cannot clear it. History from
another work or a terminal turn cannot start the status. Existing delivery and
connection warnings retain priority. The desktop does not synthesize compaction
elapsed time from history replay.


## Closing-thread recovery

At cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, app-server rejects a
resume before activation with `-32600` and an exact thread-specific `is closing;`
message. The TUI treats only that refusal as retryable. Chief now schedules the
same native resume on its retained connection after 1, 2, 4, and 8 seconds, then
at most once per minute. It does not sleep inside the coordinator or replay input.

Each retry checks the saved work, thread, turn, history revision, and dispatch
state. Archive, deletion, revert, and account suspension prevent stale recovery.
Resume requests retain native settings instead of applying new-thread defaults.
A restarted coordinator reconstructs recovery from durable unresolved work and
fresh native evidence. The schedule itself is connection-local. This batch does
not qualify ordinary-conversation recovery or a live native shutdown race.


## Ordinary conversation closing recovery

Ordinary conversations also recognize the exact thread-specific closing refusal
at upstream `595cc91e8cbb1c2ca822d0311dcf12709410c582`. Resume retries are limited
to four delays (1, 2, 4, and 8 seconds). Each attempt reacquires the existing
process-generation fence; no supervisor lock is held during the delay. Other
refusals, loss of process ownership, and uncertain replies stop retries.

Events observed before a refused resume remain buffered and accompany the next
successful resume receipt. A completion cannot disappear and make the caller
assume that an old turn is idle. Exhaustion records the exact final response
witness and an unsent-input diagnostic in the existing session. The desktop offers
refresh of that conversation with a wait-for-close message. Local protocol2.56
adds this recovery action; the database schema remains35. Fixture and database
restart tests qualify these paths; they do not prove a live native unload race.


## Permission request execution environment

At cutoff `595cc91e8cbb1c2ca822d0311dcf12709410c582`, app-server forwards the
native permissions request's optional `environmentId`. Decodex preserves that
field in the owned approval projection and displays the native identifier next
to the requested permissions. An absent or empty identifier does not produce an
inferred local environment. Numeric values cannot become approval display text.

Installed Codex `0.155.0-alpha.16.3` includes the nullable string field in its
experimental generated `PermissionsRequestApprovalParams` schema. Projection and
rendered tests cover Unicode identifiers and missing or invalid values. This is
request attribution, not proof of remote executor availability or a remote grant.
