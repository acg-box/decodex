---
type: Reference
title: "Local database operations"
description: "Local database operations"
tags: ["decodex", "architecture"]
openwiki_generated: true
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-268229e2b9f21dae93c32513
    resource: repo://crates/decodex-protocol/src/wire.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-a67672a943dfe221574b2501
    resource: repo://crates/decodex-runtime/src/shared_auth_coordinator.rs
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
  - id: openwiki-source-960cb6b925f1fa45c737a735
    resource: repo://scripts/macos/verify_decodex_bundle_contracts.py
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Local database operations

## Authority and paths

The sole normal product store is `~/.decodex/server/decodex.sqlite3`, owned by `decodex serve`. GPUI, the menu-bar library and short-lived CLI commands use the local protocol. They must not inspect or mutate SQLite directly as a fallback.

The store uses a serialized connection, owner-private paths, bundled SQLite and embedded migrations. Schema version 30 is current in this source revision. The migration ledger is authoritative; old schema-9/10/11 evidence is not a reason to rebuild or reset a user's database.

## Installation and upgrade

Select the app-bundled CLI installation or the standalone local-service installation. Do not install two competing service owners. The app includes a signed helper and native-client/menu-bar libraries; service and UI compatibility is checked. The exact local protocol is 2.43. An incompatible service is a version problem, not proof that history has disappeared.

Use `decodex --help` and `decodex serve --help` from the installed artifact before operating a host. Use the repository stage/install scripts for that installation mode. Preserve database and credential rollback sources. A documentation refresh does not authorize deletion of retained data.

The one-shot `database/transfer` tool imports legacy state through its protected fixed-source workflow. It is not part of normal startup and is not an automatic repair command.

## Account routing and recovery

Current Route is a synchronous service-owned operation. The service locks routing and the target account, reads current revisions, refreshes when required, checks shared-auth source identity and external-client liveness, performs exact projection/readback, and commits the authoritative result. A running auth-owning Codex client or source drift can reject the operation. The old Pending Route receipt, 100 ms recovery loop, and process-blocker DTO are no longer the current contract.

Same-account refresh remains serialized through the credential owner. A valid non-older shared-auth winner can be adopted instead of writing back a losing token. Never print token values while diagnosing this path.

Chief conversation ownership is a different boundary: unavailable conversations reject new input and preserve readable history. Checking availability is not a request to resend messages. See [Chief coordination](../architecture/chief-coordination.md).

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
