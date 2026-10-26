---
type: Reference
title: "Historical vNext authority decision"
description: "Current SQLite authority supersedes the historical disposable server-store design."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-dc8958560b1230967470835f
    resource: repo://database/src/schema.rs
generated: { by: "codex", at: "2026-10-26T14:39:55.302Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-26T14:39:55.302Z
---

# Product state authority

`decodex serve` owns the product database. Clients use the local protocol for product reads and writes. Native Codex owns conversation history and execution.

`database/schema.sql` defines one current SQLite schema. The service initializes an empty database directly and verifies the exact current application ID, schema version and structure. It refuses an older layout without resetting its data. Historical migration chains and account-transfer adapters are not application entrypoints.

An operator must qualify a local data conversion before switching a retained installation to another schema. Keep native thread identities, history, current input and unresolved effects separate from disposable obsolete records. A successful fresh initialization alone does not prove that retained data was preserved.

Use [Runtime architecture](../architecture/runtime-architecture.md), [Local database operations](../operations/local-database.md) and [conversation recovery](../workflows/conversations-and-recovery.md) for the current owners and checks.
