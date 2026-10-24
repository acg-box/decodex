---
type: Reference
title: "Local database operations"
description: "Service-owned SQLite upgrades, account routing and durable credential and branch records."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-d511cef2848dd5f6968572ae
    resource: repo://crates/decodex-runtime/src/agent/tool_migration.rs
  - id: openwiki-source-a09c082db4ad1473c4d1e557
    resource: repo://crates/decodex-runtime/src/application.rs
  - id: openwiki-source-2da6601c3f30e806c504e991
    resource: repo://crates/decodex-runtime/src/host_credentials.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-8076269b8760701249ad3b9c
    resource: repo://database/migrations/0052_personal_access_token_credentials.sql
  - id: openwiki-source-bdc10e06c0058b26a9bde83d
    resource: repo://database/migrations/0056_project_agent_work.sql
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-0f0fc00c258a3ddad6935fcc
    resource: repo://database/src/agent_tool_migration.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
  - id: openwiki-source-28af4781ad8d9558410630da
    resource: repo://database/src/works.rs
  - id: openwiki-source-c0eb738b42368debe7ef7545
    resource: repo://database/src/works/commands.rs
  - id: openwiki-source-960cb6b925f1fa45c737a735
    resource: repo://scripts/macos/verify_decodex_bundle_contracts.py
generated: { by: "codex", at: "2026-10-24T21:11:31.338Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-24T21:19:56.719Z
---

# Local database operations

## Project, Agent and Work migration

The current ledger ends at schema 56, `0056_project_agent_work.sql`. It applies one direct transaction under the existing digest-checked migration owner. The migration renames Workspace directory and membership data to Projects and Project Agents. It adds persistent Agent responsibilities, separate Work, immutable requirements, scoped Materials, Results and explicit reuse, Collaboration Requests and issued instructions, stable command receipts, global Guardian settings/source receipts, private observations and native tool-migration journals.

Existing conversation, inbox, native binding and genuine old execution records survive. Passive historical outcomes acquire Work owners and explicit legacy requirements. A migrated completed outcome retains a historical assessment with a no-new-qualification limit. No old executor becomes a native Subagent by renaming a row. The application does not require a reset, a second database or a dual-write migration.

Work commands validate the current owner, expected requirements, holds and related evidence inside an immediate transaction. A committed command ID replays only the exact same actor and content. Completion requires current related Results and handled required requests. Old Results stay historical unless a new reuse assessment relates them to current requirements. SQL applies Agent and granted Material scope before bounded reads.

Native contribution intent and exact direct-call identities are attribution records, not another executor or mailbox. Missing or ambiguous binding remains Unknown. The service reconciles exact native history before another external action. Peer requests use the existing inbox and exact delivered-turn receipts; restart does not imply acceptance.

Tool migration uses `reserved`, `created`, `retiring`, `unknown` and `applied` outcomes. It preserves exact old/new identities and requested settings. Positive native retirement is required before the local binding switches. The previous context remains read-only history. Uncertain creation/retirement parks the migration; database retry does not recreate a native thread.

Global Guardian stores desired reviewer preference and Review Details opt-in separately from source-specific effective receipts. Private records are deduplicated by reviewer thread, turn and item. Records are bounded to 64 KiB and the newest 128; small snapshot previews and exact selected reads stay separate. These records do not grant execution authority or complete Work.

Protocol 3.0 is the matching direct public-contract change. Old clients fail exact negotiation. The migration and runtime tests use temporary roots; this task did not migrate the user's installed database. The local gate passed schema56/89-table integrity and 188 database tests passed. See [commands](commands-and-validation.md) and [the canonical contract](../decisions/project-agent-work.md).

## Authority and paths

The sole normal product store is `~/.decodex/server/decodex.sqlite3`, owned by `decodex serve`. GPUI, the menu-bar library and short-lived CLI commands use the local protocol. They must not inspect or mutate SQLite directly as a fallback.

The store uses a serialized connection, owner-private paths, bundled SQLite and embedded migrations. Read `database/src/migrations.rs` for the current schema boundary. The migration ledger is authoritative; old schema-9/10/11 evidence is not a reason to rebuild or reset a user's database.

## Installation and upgrade

Select the app-bundled CLI installation or the standalone local-service installation. Do not install two competing service owners. The app includes a signed helper and native-client/menu-bar libraries; service and UI compatibility is checked. Clients must match `CURRENT_VERSION` from the local protocol crate. An incompatible service is a version problem, not proof that history has disappeared.

Use `decodex --help` and `decodex serve --help` from the installed artifact before operating a host. Use the repository stage/install scripts for that installation mode. Preserve database and credential rollback sources. A documentation refresh does not authorize deletion of retained data.

The one-shot `database/transfer` tool imports legacy state through its protected fixed-source workflow. It is not part of normal startup and is not an automatic repair command.

## Account routing and recovery

Current Route is a synchronous service-owned operation. The service locks routing and the target account, reads current revisions, refreshes when required, checks shared-auth source identity and external-client liveness, performs exact projection/readback, and commits the authoritative result. A running auth-owning Codex client or source drift can reject the operation. The old Pending Route receipt, 100 ms recovery loop, and process-blocker DTO are no longer the current contract.

Same-account refresh remains serialized through the credential owner. A valid non-older shared-auth winner can be adopted instead of writing back a losing token. Never print token values while diagnosing this path.

Agent conversation ownership is a different boundary: unavailable conversations reject new input and preserve readable history. Checking availability is not a request to resend messages. See [Agent coordination](../architecture/chief-coordination.md).

## Validation without touching production data

```sh
python3 scripts/vnext/local_database_gate.py
cargo +stable test -p decodex-database --lib
cargo +stable test -p decodex-runtime --lib
scripts/macos/test_decodex_app_stage.sh
```

The database gate and unit tests use isolated fixtures. The staging test checks the signed bundle's native compatibility. These do not authorize migration experiments on the user's root.

## Related operations

- [Reset Cards](reset-cards.md): explicit redemption and durable uncertain outcomes.
- [Weekly quota activation](quota-activation.md): deduplicated minimal subscription request.
- [Commands and validation](commands-and-validation.md): active tools and evidence boundaries.

## Current migration boundary

The embedded migration ledger defines the baseline and ordered upgrades. The verifier checks migration identity and compatibility; old numbered migration files in historical receipts are not an instruction to reset or reconstruct user data. O24 retirement changes notification recording, not the schema or readability of saved events. See [accounts](accounts-and-routing.md).

## Credential and branch records

OAuth credentials retain their version-1 payload and fingerprint representation. PAT credentials use version 2 and retain verified user identity without fabricated OAuth refresh or expiry values. Migration 52 rebuilds the credential and process-generation schema checks to accept both versions. The migration copies existing values, recreates the Agent process authority trigger, checks foreign keys before commit and restores foreign-key enforcement on both success and failure. Use the migration owner; do not edit these tables manually.

Explicit conversation branches reuse durable inbox event records for their intent, native acknowledgement and observed prefix. The target work and native thread are independent from the source. Before-input branches then reuse the existing canonical prompt-edit draft and upload records. Unknown creation does not authorize a second request. See [conversation recovery](../workflows/conversations-and-recovery.md).
