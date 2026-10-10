---
type: Reference
title: "Historical vNext authority decision"
description: "Current SQLite authority supersedes the historical disposable server-store design."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
generated: { by: "codex", at: "2026-09-29T13:52:19.644Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-10T06:49:11.265Z
---

# Current authority supersedes this design

The former server-store design remains in Git history. Its disposable-data and no-migration directions must not be used on the current product database. Current code embeds ordered SQLite migrations, verifies their digests, and preserves user state. The executable owner is `decodex serve`; historical server-store and repository-orchestration paths are not current entrypoints.

Use [SQLite decision](sqlite-local-product.md), [Runtime architecture](../architecture/runtime-architecture.md), and [Local database operations](../operations/local-database.md). The [historical decision](https://github.com/acg-box/decodex/blob/426e862a5978620ffad65fb3d0189cfa57392873/openwiki/decisions/vnext-authority.md) explains the superseded design. Its benchmarks and acceptance results are not current validation.
