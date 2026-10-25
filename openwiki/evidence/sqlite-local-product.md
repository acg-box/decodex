---
type: Reference
title: "SQLite local product evidence and current verification boundary"
description: "Current source owners and the limitations of retained desktop and storage evidence."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-acf49c93c3e80379f0023c71
    resource: repo://apps/decodex-gpui/src/accounts.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-10-21T09:01:22.045Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T06:52:57.034Z
---

# Current verification boundary

The detailed receipt below describes the older desktop consolidation, not the current build. Its protocol 2.11, cohort 7, schema 11, decodexd helper, Factory UI, and statement that Reset Card GUI consumption is absent are superseded.

Read `CURRENT_VERSION` and the embedded migration ledger for the current protocol and schema. SQLite retains durable Reset Card operations and quota activation. The package contains `Contents/Helpers/decodex`; Accounts and CLI now expose service-owned Reset Card operations with account-scoped recovery. `scripts/macos/test_decodex_app_stage.sh` verifies bundle identity, signatures and native ABI, including a negative mismatch fixture. These source-defined checks are not a fresh live acceptance receipt from this documentation run.

Current Route is synchronous and credential-negative. The service returns authoritative completion or refusal; the historical Pending Route process-list DTO is no longer present. Desktop clients bind the result to their active session and exact command.

See [Runtime architecture](../architecture/runtime-architecture.md), [Reset Cards](../operations/reset-cards.md), and [Commands and validation](../operations/commands-and-validation.md).


## Historical receipt

The [original desktop consolidation receipt](https://github.com/acg-box/decodex/blob/426e862a5978620ffad65fb3d0189cfa57392873/openwiki/evidence/sqlite-local-product.md) remains in Git history. Its old test counts, schema inventory, capture results and locked-console limits apply to that revision. They are not current validation or a requirement to restore the retired UI.
