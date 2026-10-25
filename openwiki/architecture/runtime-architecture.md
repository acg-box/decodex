---
type: Reference
title: "Runtime architecture"
description: "Service ownership, fixed Codex components, shared acquisition and app updates."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-d700ef551f46158044378d8f
    resource: repo://apps/decodex-cli/src/lib.rs
  - id: openwiki-source-0a61b25348131fe1bb6e1d12
    resource: repo://apps/decodex-gpui/src/agent_context_settings_wire_tests.rs
  - id: openwiki-source-c76550e85b25876b339f9255
    resource: repo://apps/decodex-gpui/src/agent_context_settings.rs
  - id: openwiki-source-ae991159453be2ee0c611ac1
    resource: repo://apps/decodex-gpui/src/bundled_daemon.rs
  - id: openwiki-source-c1d5f763b821a11957a8c33b
    resource: repo://apps/decodex-gpui/src/client_lifecycle.rs
  - id: openwiki-source-cfb92e5db8b3a39d51f6a057
    resource: repo://apps/decodex-gpui/src/settings_updates.rs
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-2b08587e9b26e203156110c8
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences.rs
  - id: openwiki-source-94c4593cb5944bf1f2ecf050
    resource: repo://crates/decodex-codex/src/app_server_client/dispatch_refusal.rs
  - id: openwiki-source-0d4c68456f2e50c6fc7ad5eb
    resource: repo://crates/decodex-components/src/install.rs
  - id: openwiki-source-2d990e4e2bc233a95b4894b9
    resource: repo://crates/decodex-components/src/pin.rs
  - id: openwiki-source-6230c010baca677fa60c32c1
    resource: repo://crates/decodex-protocol/src/client.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-1f19769e3cb97a2389e92f82
    resource: repo://crates/decodex-runtime/src/account_launch.rs
  - id: openwiki-source-9e21e877b2d7ad96b0a4d637
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_capacity_tests.rs
  - id: openwiki-source-f5d073da07bcb17ee416f3b5
    resource: repo://crates/decodex-runtime/src/account_launch/process_native_control_tests.rs
  - id: openwiki-source-b2869e6da778a74cb6afc667
    resource: repo://crates/decodex-runtime/src/account_launch/process.rs
  - id: openwiki-source-4e8482c0f2dbe370858a1b11
    resource: repo://crates/decodex-runtime/src/agent_context_settings_native_tests.rs
  - id: openwiki-source-f2860995fd89f9170b5406cb
    resource: repo://crates/decodex-runtime/src/agent_context_settings.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-a4a2658e06fa2cf48ff4589e
    resource: repo://crates/decodex-runtime/src/agent/global_guardian.rs
  - id: openwiki-source-52d3ef4824079f66e9063566
    resource: repo://crates/decodex-runtime/src/agent/native_subagents.rs
  - id: openwiki-source-30c7edf894a639091ecbafc6
    resource: repo://crates/decodex-runtime/src/agent/tests/external_context.rs
  - id: openwiki-source-b1dd4c8ab85f342314daa6f1
    resource: repo://crates/decodex-runtime/src/agent/work.rs
  - id: openwiki-source-a958837c2596362a3517c745
    resource: repo://crates/decodex-runtime/src/websocket.rs
  - id: openwiki-source-6085af9603cb7ac0ff3ef1cf
    resource: repo://crates/decodex-runtime/src/work_portfolio.rs
  - id: openwiki-source-07c9cc033899826079f8efb5
    resource: repo://crates/decodex-runtime/tests/native_qualification/large_catalog.py
  - id: openwiki-source-88b81a215b1bac061dad80ec
    resource: repo://crates/decodex-runtime/tests/native_qualification/model_errors.py
  - id: openwiki-source-28af4781ad8d9558410630da
    resource: repo://database/src/works.rs
  - id: openwiki-source-87eeaebc39a3d2229938f9d2
    resource: repo://scripts/macos/build_codex_source.mts
  - id: openwiki-source-e8c30c6f5849a5077d7b4aa5
    resource: repo://scripts/macos/build_codex_source.test.mts
  - id: openwiki-source-58e225584250806d8658ea09
    resource: repo://scripts/macos/prepare_codex.mts
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-10-24T21:11:31.338Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T01:55:03.097Z
---

# Runtime Architecture

## Process and state ownership

The current service entrypoint is **`decodex serve`**, from `apps/decodex-cli`. There is no active `apps/decodexd` workspace member or packaged `decodexd` helper. The same executable supplies short-lived CLI commands.

The service owns the local protocol listener, SQLite product state, credentials, account routing and observations, process generations, provider attempts, conversation bindings, Agent coordination, and explicit external effects. The fixed database is `~/.decodex/server/decodex.sqlite3`. Clients do not open the database or auth files as a fallback.

```mermaid
flowchart LR
    UI[GPUI workspace] --> Client[Typed local clients]
    Menu[In-process Swift menu bar] --> FFI[Native client library]
    CLI[decodex commands] --> Service[decodex serve]
    Client --> Service
    FFI --> Service
    Service --> DB[(SQLite)]
    Service --> Codex[Codex app-server]
```

## Application and bundle

`Decodex.app` is the macOS GUI. It contains `decodex-gpui`, signed `Contents/Helpers/decodex`, a native-client FFI library, and `libDecodexMenuBar.dylib`. The menu bar and attached glass controls belong to the same app process. They are not extra apps or state owners.

For local profiles, the app supervises its bundled helper with `serve --parent-fd`. The local service boundary prevents a second owner from replacing an existing service. Typed clients reject incompatible protocol versions instead of opening product storage directly. Closing the main window differs from quitting: the former preserves the running app; quitting retires only its owned service.

The GUI installer links the user CLI to the bundled helper. The standalone service installer supplies a regular executable instead. These are alternative installation modes.

## Native runtime and coordination

Codex app-server owns provider threads and execution. Decodex adds durable work relationships and protocol projections. Agent shares a retained account-bound process across its related work; ordinary conversation sessions keep their own runtime/account bindings. Historical Program aggregates are a read-only compatibility surface, not a restored execution lane. Native child approvals resolve through verified ancestry. Account routing, subscription usage, Reset Card redemption, and weekly activation stay service-owned.

Decodex resolves child requests through the verified native parent chain. The integration does not maintain a second child-admission lifecycle.

Retired repository/GitHub effect orchestration does not return through a wiki update. Historical records remain readable; a historical operation shape is not live execution authority.

## Project, Agent and Work boundary

Project and Work are durable business records. A persistent Agent owns one current native conversation and can own several Works. Assistant and Project Lead select responsibilities of existing Agents. `work_read` and `work_manage` expose scoped evidence and atomic decisions. They do not create an execution engine. Native helpers use native spawn, messages, waits and continuation; independent Agents exchange scoped Collaboration Requests through the existing inbox.

The overview contains Work headers. `work_portfolio` serves selected evidence in digest-checked 8 KiB pages with a 32 MiB ceiling. SQL applies Agent and granted Material scope before the per-category record limit. The global Guardian coordinator records desired reviewer preference and actual per-source application receipts. It does not own execution approval or business assessment. Optional private records exclude raw reasoning and preserve explicit missing action associations.

Direct schema 56 migration preserves genuine legacy execution and history. New conversations advertise the canonical tool revision. Existing tool sets use a durable migration journal and exact native quiescence/readback. Unknown external effects remain parked. Previous thread history is read-only evidence, not input authority. See [coordination](chief-coordination.md) and [local database](../operations/local-database.md).

## Native launch policy

Decodex constructs and attests one fixed app-server argument list. It enables native tool-description-first ordering, native subagent context defaults, Agent message wakeups, and full-fork prefix preservation. The dotted prefix option preserves the existing `multi_agent_v2` selection; it does not enable multi-agent execution by itself. Code Mode hosting transport stays under native configuration.

The embedded runtime requirement selects the exact component. Development uses a reviewed upstream commit; production selects a qualified official Release. Check the actual component and its generated protocol schema before claiming that an API is available. See [upstream maintenance](../operations/codex-upstream-autopilot.md).

## Startup preflight cleanup

Version and schema preflights reserve one slot in the daemon's bounded cleanup quarantine before the first child starts. `ProcessGuard::Preflight` retains the shared capacity owner and that slot through both preflights. It does not grant account authority or return a `RunnerPermit`. Account-bound processes keep their existing permit.

If immediate cleanup cannot finish, `ProcessGroupOwner` transfers the child, its output pump and the guard to the reaper. The guard remains owned until cleanup completes. Existing queue limits, preflight deadlines and process-group signaling rules still apply. The two-phase profile regression covers cleanup handoff during either version or schema preflight.

## Context defaults

Task preferences > Context defaults exposes two native settings. The panel reads the saved user value and the effective project default through the current account-bound app-server. It writes one boolean with the native file path and version from that observation, then reads the result again. It does not store a second preference in SQLite or add a launch argument that forces either setting on.

The same values can be edited in native `config.toml`:

```toml
[features]
async_compaction = true
retain_client_tool_outputs = true
```

`false` explicitly disables a setting. An absent value keeps the native default, which is off for these two experimental features. Project or managed configuration can override a saved user preference. The panel reports both values when they differ. A changed file or source requires a fresh review; an unconfirmed save is not retried automatically.

To set the native async threshold, replace its boolean with the table form:

```toml
[features.async_compaction]
enabled = true
model_async_compact_threshold_percent = 65
```

Changing the On or Off choice preserves an existing threshold table. Defaults apply to new conversations. Loaded conversations keep their configuration; the service does not restart or resume them to apply a preference.

Background compaction prepares a native summary before context fills. Native runtime eligibility and thresholds decide whether work starts. An enabled configuration value does not prove that a running conversation is using background compaction, or that hosted latency or cost improved.

Compatibility tool-output retention selects only Decodex delegation instructions. The producer sends `retain: true` for `work_instruction` while preserving its tool authority. Codex decides whether to keep it under `retain_client_tool_outputs` and its retention budget. Ordinary results, wakes and retries do not all become permanent context. See [Agent coordination](chief-coordination.md#native-contributions-and-independent-collaboration).

## Components and app updates

`decodex-components` owns one per-user component store, package verification and cache repair. The GUI and service embed the same runtime requirement. App startup prepares that component before launching the owned service; preparation does not consume connection retry attempts. Settings > Updates & Components shows the selected component, progress, errors and Retry. Sparkle owns the separate Decodex application update flow.

Official Release inputs require an exact tag, canonical OpenAI asset URL, archive SHA-256, executable SHA-256 and generated schema fingerprint. The installer checks the archive before extraction, then verifies signatures, version and protocol evidence. Downloads have three bounded attempts. A verified replacement is published atomically; a failed repair preserves the previous directory, and other versions remain available.

Development keeps [the reviewed source requirement](../../codex-runtime.lock.json). `cargo make prepare-codex` explicitly builds or imports its qualified artifact. The source builder uses the common checked download path, stable Rust and the unchanged upstream Cargo lock. Its extractor refreshes source modification times while retaining the dependency build cache, so historical archive timestamps cannot select stale local objects. The app does not compile Codex.

Development artifacts use the same package verification and installation path. A local receipt binds archive and executable hashes to the fixed source commit and expected schema. CLI and Code Mode helper commit checks reject a different revision. A new artifact becomes selectable only after its installation verifies. Hashes and schema are integrity evidence; affected native regression tests still establish behavior.

The [portable qualification record](../../automations/upstream/qualification.json) binds the current artifact, helper hashes, source checks and native consumer results to their limits. The [retained evidence index](../../automations/upstream/retained-evidence.json) supplies the reusable requirements and commands. The native automation directory owns detailed receipts and the live review cursor. A source-test launcher that enables private fixture hooks is separate from the production CLI; neither source-test success nor this record establishes installed-app acceptance.

The current qualification retains terminal `modelNotFound` classification through live, stored and cold history without native retries or Decodex capacity retries. It also checks that an explicitly configured model catalog larger than 1 MiB reaches native model discovery. Reusable fixtures belong to `crates/decodex-runtime/tests/native_qualification/`; these synthetic provider cases do not add an account type or a provider setup control.

Production staging selects a separate official Release requirement. Release pinning requires the Release to contain the current development baseline, and behavioral qualification must cover retained fixes. An older official Release is not a fallback. Packaging includes the requirement and Sparkle, but no Codex binary or schemas. See [Commands and validation](../operations/commands-and-validation.md).

## Protocol and safety

Clients use same-UID local transport and exact protocol/artifact compatibility. Protocol 3.0 carries Project, Agent, Work and global Guardian. An incompatible client fails explicitly; there is no parallel alias rollout. Publication-instance receipts remain enabled for this generation, including reconnect and idempotent command recovery. Credentials do not enter normal product projections. Revisions and stable command identities guard mutations. An unknown outcome requires authoritative readback, not blind retry.

Native shutdown admission errors use code `-32600` and the structured `serverShuttingDown` reason. The older exact-message fallback applies only when error data is absent. A classified refusal alone never authorizes replay: callers must establish exact request identity and no prior effects.

The Settings menu-bar preference is durable SQLite state. macOS launch-at-login registration is a distinct platform preference. Window material, focus, animation and local panel visibility remain presentation responsibilities.

## Packaging and verification

Select full Xcode with `xcode-select`; scripts honor an explicit `DEVELOPER_DIR` override. They do not require a fixed Beta path. Rust compilation uses stable. Run:

```sh
cargo +stable test -p decodex-runtime -p decodex-database -p decodex-codex --lib
cargo +stable test -p decodex-gpui --bin decodex-gpui
scripts/macos/test_decodex_app_stage.sh
```

The stage test verifies signed bundle shape, metadata, native ABI compatibility, and a deliberately incompatible fixture. It does not prove microphone permissions, visual quality, or live provider acceptance. See [Commands and validation](../operations/commands-and-validation.md), [Agent coordination](chief-coordination.md), and [Desktop workspace](desktop-workspace.md).
