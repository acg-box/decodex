# Codex integration review: 2026-09-17

## Review boundary

Decodex base: `579b8d74fe57b2915e3695ad76fd519ea7bbf4f5`.
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
| Account limits and model discovery | Current account and model consumers tolerate new account routing, ordinary-usage, upsell and access-program metadata. Auth callback identity requirements remain checked by the real schema loader. Optional provider/workspace products are not enabled by accepting unrelated metadata. |
| Sandbox and approvals | Current installed/main experimental shapes used by Decodex remain compatible. The retained bridge adds only the two read-only history methods. Account mutation and unowned approval responses remain rejected. Approval and user-input requests retain exact IDs and require an explicit owner response. |
| Native collaboration | The actual generated schema passes Decodex's collaboration checks for installed, stable and current main. Chief continues to own independent threads and durable work relationships; upstream subagent APIs do not replace that product owner. |
| Messages, usage and compaction | Current native history preserves assistant Markdown text, phase, async delivery and other item metadata. Token-usage notification shape is unchanged between installed and main. New raw-response usage metadata is an internal notification, not a replacement for current turn terminal events. Compaction remains owned by Codex. A new token-usage dashboard is not implemented by this compatibility patch. |
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

`cargo make check` is not green: the unchanged site dependency graph fails
`npm audit` with eight high and one critical finding. An additional account-login
architecture check has two unchanged source-text assertions for obsolete
`AccountLoginRequest::Status` and `AccountLoginRequest::Cancel` spellings. The
relevant test and source files match the reviewed base. These failures are not
claimed as passing or repaired by this Codex adaptation. No dependency, workflow,
OpenWiki, live account or schema migration changes are included.
