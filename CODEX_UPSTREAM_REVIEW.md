# Codex integration review: 2026-09-17

## Review boundary

Initial Decodex base: `579b8d74fe57b2915e3695ad76fd519ea7bbf4f5`.
Follow-up base: `d0b20dbc32bcfd3952a938a74aadb4544a3fc981` (merged PR #1337).
Official Codex main: [`c5d079470eeaf9502080faa0697aade481242081`](https://github.com/openai/codex/commit/c5d079470eeaf9502080faa0697aade481242081).
Stable release: [`rust-v0.154.0`](https://github.com/openai/codex/releases/tag/rust-v0.154.0),
commit `6b9826e3aa83b1a5947db50f4332cb9c65f1b340`.
Installed test binary: `codex-cli 0.154.0-alpha.6.2`.

This review establishes a current integration baseline from source, official
experimental schema exports, the installed binary's generated schema, and tests.
It does not certify that all 1,445 historical commits after the old August 18
cursor were individually reviewed. That cursor was not accepted as proof of
current compatibility. The current Conversation and Chief implementations
supersede the old Quick Task implementation and PRs #1300 and #1301.

## Changes delivered

| Upstream change | Decodex consequence | Adaptation and evidence |
| --- | --- | --- |
| [`c62d191c4c`](https://github.com/openai/codex/commit/c62d191c4c8c0cab7045fca6efc399197334bb6c): `thread.rs`, `turn.rs`, thread processor and resume/fork tests | New `disabledPluginIds` response fields fail the strict Conversation start/resume decoder. | Accept the actual string-array field, default it when omitted by older servers, and reject malformed values. Upstream main only relative to the tested stable release. The field does not yet enforce plugin filtering. |
| [`91d54f1667`](https://github.com/openai/codex/commit/91d54f1667e627538db9d44d2ce88a260b4213b0): resume protocol, thread processor, persisted/legacy collaboration-mode tests | The new `collaborationMode` response field fails Conversation resume decoding. | Decode the optional typed mode/settings object, including snake-case settings. Preserve existing model, effort, cwd, and permission checks. Upstream owns restoration of saved mode. Main only relative to the tested stable release. |
| [`5cb7a35de9`](https://github.com/openai/codex/commit/5cb7a35de938e2475e5c1c088f111915008fd100), [`d132b69219`](https://github.com/openai/codex/commit/d132b692199c53c085c7b2cbec3c44e2dc5cf277): native history list APIs, thread processor, `thread_read.rs` and `thread_resume.rs` tests | Chief completion and recovery load the whole thread and use deprecated full-history hydration for paginated threads. | New Chief/worker threads select paginated history. Resume requests exclude turns. Exact result reads page turns and items, preserve item order and metadata, and reject repeated cursors, wrong identities, and exhausted bounds. Existing legacy threads retain their supported read path. This capability is released and was tested with the installed binary. |

The page reader has a 60-second deadline, 128-page limits, and an 8 MiB aggregate
page budget. A missing turn remains missing. An incomplete read returns an error;
it does not invent a complete result or grant dispatch replay authority. Chief
still records positive terminal evidence separately from result-read failure.

## Current integration coverage

| Surface | Sources and current conclusion |
| --- | --- |
| Initialization, transport and process ownership | Current `AppServerClient` and account-process bridge were compared with official request/notification exports. Stdio and the initialize/initialized handshake remain supported. Multiplexing, exact response IDs, event overflow, disconnect handling, and explicit process ownership have local regression coverage. |
| Thread start/resume/read/list/archive | Compared current and installed experimental exports for these methods and nested Thread/Turn definitions. The response additions above were the uncovered start/resume fields. Current Decodex already accepts project, model, effort, originator, environment and Daybreak metadata. Read/list projection tolerates additive fields. |
| Turn dispatch, steering, interruption and recovery | Current turn request/result shapes remain compatible. Chief preserves independent thread and active-turn identity and does not replay uncertain submissions. Pagination tests cover missing exact turns and cross-turn result rejection. |
| Authentication | `ChatgptAuthTokensRefreshParams` and response shapes are unchanged in installed, stable and main exports. The copied login source has an explicit baseline, `9392c3fa5bcda342b5b96a1a04d67b2f781617c2`. Comparing its four cited source files with current main found only an added optional Bedrock storage field and its `None` initializer. Browser/device flow and PKCE logic are unchanged in that bounded source scope. Current local refresh classification already handles HTTP 400 `invalid_grant` as rejection. |
| Account limits and model discovery | Account responses accept additive metadata. This does not implement model discovery: the product uses a static model picker and free-text IDs. Account quota display uses the direct account API, not the new app-server usage capability handshake. See the open gaps below. |
| Sandbox and approvals | Current installed/main experimental shapes used by Decodex remain compatible. The retained bridge adds only the two read-only history methods. Account mutation and unowned approval responses remain rejected. Approval and user-input requests retain exact IDs and require an explicit owner response. |
| Native collaboration | The schema check alone missed semantic losses: four v2 tool names, interrupted tool calls, and completed child activity normalized to Unknown. This follow-up adds their typed classifications. Chief still owns independent work threads; a native child completion does not resolve Chief work. |
| Messages, usage and compaction | The earlier patch preserved stored metadata but missed delivery timing. This follow-up records async assistant messages before terminal completion, public token counters, and completed compaction observations. It does not implement interactive question cards or mid-turn user reply delivery. Internal raw-response usage metadata is not interpreted as price. |
| Removed or optional features | Decodex has no `thread/rollback` consumer. Its removal does not require a local compatibility alias. TUI-only controls, new provider onboarding, remote-control services and optional plugin-management APIs do not require ports into the active integration. |

The principal source owners are `codex-rs/app-server-protocol/src/protocol/v2/`,
`codex-rs/app-server/src/request_processors/thread_processor.rs`,
`codex-rs/app-server/tests/suite/v2/thread_read.rs`, and the four login files named
in `crates/decodex-account-login/THIRD_PARTY_NOTICES.md`.

## Validation

- `cargo +stable test -p decodex-codex -p decodex-runtime -p decodex-account-login --all-targets --all-features`: passed, including pagination, strict metadata, durable recovery and account tests.
- Clippy for `decodex-codex` and `decodex-runtime`, all targets/features: passed with warnings denied.
- `official_schema_supports_current_consumers`: passed separately against installed, stable-release and current-main experimental exports. Set `DECODEX_REVIEW_SCHEMA` to the export directory and explicitly run the ignored test.
- Installed app-server initialization: passed.
- Installed real two-thread smoke: passed. Each test thread completed a small read-only request; native pages returned its exact assistant result. Both created threads were archived. The initial ephemeral-thread attempt was rejected because Codex does not support history pages for ephemeral threads; the corrected test uses persistent test threads.
- CLI/service build, local database gate and vNext architecture tests: passed.
- Repository-pinned formatting and `git diff --check`: checked for the changed code.

The user has retired the website. Site dependency and site build checks are outside
this maintenance scope. The earlier site audit failure is not an outstanding
Codex adaptation. OpenWiki remains unchanged. An additional account-login
architecture check still contains two obsolete source-text assertions for
`AccountLoginRequest::Status` and `AccountLoginRequest::Cancel`; the active login
runtime tests passed. No live product database or account configuration was changed.

## Behavior follow-up

The follow-up compares the old claimed snapshot with current source contracts,
request processors, and active Decodex callers. These decisions are distinct from
an exhaustive review of each historical commit. The old consecutive commit cursor
remains unverified; do not advance it to the current head on this evidence alone.

| Capability and upstream evidence | Actual Decodex behavior and decision |
| --- | --- |
| Async messages and structured questions: `fb356f3d2c`, `2c79ee6dac`; core `tools/handlers/request_user_input_async.rs`, `tools/spec_plan.rs` | Upstream emits an `agentMessage` with `delivery: async`, rendered text and structured questions, then continues. It is model-catalog gated and is not a pending JSON-RPC request. Fixed immediate saved/displayed messages with exact thread/turn ownership, deduplication and restart persistence. Ordinary text replies still queue for the next Chief turn; interactive reply delivery remains open. |
| Token usage: `5f79a92e39`, `2c4a95736b`, `e017e93ace`; protocol `thread.rs` and `thread_data.rs` | Added durable public `thread/tokenUsage/updated` observations, live history display and terminal receipt metadata. Show last **response** counters separately from cumulative **thread** counters. Missing context capacity stays unknown. No fabricated context occupancy, costs or quota readiness. The two-thread installed-server test now requires valid, same-turn usage before completion. |
| Compaction and retained answers: `5971d42847`; core compaction and item projection | Codex owns compaction and retention of verified answers. Show completed compaction in Chief history without creating pending work or triggering a new model turn. No Decodex-side rewrite of native history. |
| Approval context: `9c9675d3d0`, `eb078b4f44`; protocol `item.rs`, `permissions.rs` | Fixed projection of `kind: writeStdin`, additional/network permissions, proposed policy amendments, nullable decisions, and permissions-request cwd. Old missing kind defaults to command, matching upstream. Exact callback/event ownership and explicit responses remain required. No path normalization of target-native cwd. |
| Misalignment and rate-limit errors: `7276d67081`, `e0c727de04`; `thread_data.rs`, `shared.rs`, upstream `misalignment_policy` tests | Display saved error message and substantive explanation. Do not automatically submit the suggested continuation. Chief's generic terminal error storage accepts new classifications. A purpose-built continuation UI and auth-recovery progress display remain open. |
| Native agent v2: `4fa6ad1730`, `b705b6b076`; protocol `item.rs` | Fixed `sendMessage`, `followupTask`, `interruptAgent`, `listAgents`, interrupted tool status and completed child activity classification. Use `agentThreadId` for the activity target and record the containing thread separately. Source emission in core `session/mod.rs` and `multi_agents_v2` shows that the containing thread can be the initiator or a peer; it does not prove a parent edge. Preserve redaction. These are native actor facts, not Chief work acceptance. |
| Model discovery/access programs: `e3a52b87b2`, `94967e03e5`; `catalog_processor.rs`, `model_list.rs` tests | **Open product gap.** The current static picker does not reflect the selected account's paginated catalog, efforts, modalities, service tiers, retirement guidance or authorized access programs. Free-text model IDs allow selection but do not replace discovery. Do not advertise an absent/null access program as denied or granted; do not auto-select a different model. |
| Thread attachment records: `3319d9b296`; `thread_attachments.rs` processor/tests | Main-only in the reviewed exports; absent in installed/stable. Stores JSON by thread/type/identity, supports unloaded reads and idempotent add/remove, and can be unsupported by the backing store. This is a useful future PR/artifact association API, **not file upload or model input**. Decodex has no corresponding attachment product owner; do not mirror its work database into Codex. |
| Image file references and standalone tool output: `7b8b17b97a`, `e56e4922eb`; protocol `turn.rs` | Image inputs now accept `fileId` as an alternative to inline URL. Text input remains supported. Chief's composer is text-only; file/image input and externally supplied standalone tool results are unimplemented capabilities. They require an actual input/result flow, not merely an unused allowlist entry. |
| Plugin reconciliation: `bfa9646787`, `5918c743f3`; `plugins/reconcile.rs`, `plugin_reconcile.rs` tests | Installed and stable expose reconciliation. It reports changes in this pass, including removals and failed materializations; it is not proof of runtime readiness. Upstream refreshes loaded hooks. Decodex has no plugin settings/reconcile UI; this remains a product gap rather than a port of upstream bundle internals. |
| Disabled plugins and app tool exposure: `c62d191c4c`, `0ec375eb70`, `a6d4741d39`; `thread.rs`, `turn.rs`, `config.rs` | Main adds saved disabled IDs and per-app/per-account settings. The disabled-ID contract explicitly says it does **not yet filter capabilities**. Metadata decode is fixed in #1337. Do not ship a misleading disable switch. Future settings must distinguish saved preference, actual filtering, reconciliation and loaded runtime readiness. |
| MCP state, UI and elicitation: `343074d420`, `8f31b64c7f`, `7a6f469dcf`, `b71af39fe6`, `eec4a23cb1`, `097825f75a`, `a1dc95d5af`; `mcp.rs`, `item.rs` | MCP discovery failure differs from an empty catalog; runtime state differs from advertised capabilities. App UI metadata and scoped resources require a renderer/resource owner. Chief currently exposes four pending request methods; MCP forms and native verification are not among them. This is an open interaction gap, not proof that MCP forms work. |
| User verification: `ad931a45b2`, `555b82afa9`, `82d4a98912`, `7b491281c8`; `user_verification.rs` | Experimental enrollment/status/verify/delete/cancel and public-key metadata require a native verification UX. No Decodex consumer exists. Do not auto-enroll, auto-answer, or enable opt-in transport as a compatibility fix. |
| Live settings: `9695e71519`, `9112564114`, `ed42068c45`; `turn.rs`, turn processor | `turn/settings/update` affects later captures in one matching live turn; `applied` does not prove another inference occurred. Per-turn tier overrides do not change thread defaults. Chief root settings are currently immutable. Live model/effort/reviewer/tier controls remain a product gap. |
| Account usage: `577a4fcd06`, `5037919777`, `79b04f1ab5`, `a4354e2d27`; account processor | New usage-read capabilities default false. Ordinary usage permission is account/user-validated and must not be inferred from percentages. Decodex does not advertise Luna Reserve fallback. Existing direct quota display remains separate; adopting fallback or backend upsell needs account-bound evidence and is not implemented here. Workspace routing is upstream-owned; accepting metadata does not select another endpoint. |
| Managed config/provider policy: `1aaa453ce2`, `a20092a7a2`, `ce950dcf26`, `b27a6321fa`; config and turn processor | Upstream enforces provider definitions/login restrictions and adds developer/application requirements. Decodex does not write these settings. Admission errors must remain visible; empty allowed-login methods do not mean unrestricted. Browser/computer policy additions apply to optional clients Decodex does not implement. |
| Thread identity, history and provenance: `5cb7a35de9`, `d132b69219`, `986ff1cc7c`, `728cb12fe5`, `2b554fd3f9`, `196964ef10` | #1337 adopts exact paginated history and preserves existing strict start/resume identity checks. Configured thread model is not per-turn telemetry; environment selection is not connection health; root-turn attribution does not replace execution turn identity. |
| Other native-owned changes: realtime timeline/attachment, project recency, memory v2 readiness, interrupt hooks, rollout compression, Bedrock setup, Windows sandbox implementations, feedback prompt hash, Guardian attribution | Reviewed current public contracts and local callers. Decodex has no realtime, native project, memory-admin, Bedrock or Windows setup client. Core memory/hooks/review execution stays in the installed Codex process. Compression acknowledgment is not completion. No client call is added without a corresponding product behavior. |
| Removed/deprecated controls: rollback, detached review, personality | No active rollback/detached-review/personality control requires a compatibility alias. A separate review should use a separate thread when that product flow is added. |

## Remaining adaptation queue

These are assessed gaps, not completed work. Resume here alongside the unread
historical commit queue; do not replace this list with a single "reviewed head".

1. Account-bound model catalog and model/effort/tier selection, including managed
   policy and capability availability. Replace the static picker through the
   real account/process owner, not an unrelated global Codex process.
2. Complete user interaction: durable exact-turn reply delivery, structured async
   questions, and supported MCP elicitation. On lost steering acknowledgment,
   retain uncertainty rather than silently duplicating input in another turn.
3. File/image input and artifact association. Keep model input distinct from
   main-only stored attachment records; test capability absence on released Codex.
4. Plugin/app settings and reconciliation. Prove effective behavior before
   presenting a control as active; track disabled-plugin enforcement upstream.
5. Live execution settings and provider recovery UI. Use the actual live-turn
   semantics and retain pending approval identities.

## Follow-up validation

- All-target/all-feature tests for Codex, database and runtime packages pass.
- Clippy for those packages passes with warnings denied.
- Real installed dual-thread smoke passes, including same-turn usage before
  completion and exact native history reads. It does not exercise every optional
  model-gated async tool or main-only API.
- Database migration 15 adds two history indexes. A disposable version-14 upgrade
  test preserves existing messages and pending disposition. The database gate
  passes at schema 15 with 47 tables. No existing migration was rewritten.
- Regression tests cover observation deduplication, persistence after reopen,
  absence of work/wake side effects, visible async text, terminal usage, approval
  context, failure explanations and native v2 event classification.
- vNext architecture checks pass. Website checks are excluded by the user's
  retirement decision, not reported as passing.
