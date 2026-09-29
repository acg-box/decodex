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
  - id: openwiki-source-a09c082db4ad1473c4d1e557
    resource: repo://crates/decodex-runtime/src/application.rs
  - id: openwiki-source-2da6601c3f30e806c504e991
    resource: repo://crates/decodex-runtime/src/host_credentials.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-8076269b8760701249ad3b9c
    resource: repo://database/migrations/0052_personal_access_token_credentials.sql
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
  - id: openwiki-source-960cb6b925f1fa45c737a735
    resource: repo://scripts/macos/verify_decodex_bundle_contracts.py
generated: { by: "codex", at: "2026-09-29T13:52:19.644Z" }
verified:
  - by: openwiki/0.6.1
    at: 2026-09-29T20:08:07.145Z
---

# Local database operations

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
