---
type: Reference
title: "Historical server-store feasibility"
description: "Retired storage prototype evidence and the current SQLite and blob boundary."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-9cb0f6612fed4591dd1bd8d3
    resource: repo://crates/decodex-core/src/blob.rs
  - id: openwiki-source-fe721c16e120e74e1b8f9702
    resource: repo://database/Cargo.toml
  - id: openwiki-source-0184655e2e99000280ce7bbe
    resource: repo://database/src/lib.rs
generated: { by: "codex", at: "2026-10-21T09:01:22.045Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-21T09:01:22.045Z
---

# Current scope

The former server-store prototype below is retired. Normal product storage is bundled SQLite; do not install a separate database server or reproduce the old pool/migration setup for the current application. Blob identity and disposable UI caching remain distinct owners.

This documentation refresh does not rerun or broaden the dated evidence. See [Runtime architecture](../architecture/runtime-architecture.md) and [Commands and validation](../operations/commands-and-validation.md) for current work.


## Historical receipt

The [XY-1264 feasibility receipt](https://github.com/acg-box/decodex/blob/426e862a5978620ffad65fb3d0189cfa57392873/openwiki/evidence/vnext-storage-feasibility.md) remains in Git history. Its prototype cluster, dependency choices, benchmark numbers and removed proof command apply only to that experiment. They do not define current storage operations or require users to install its database stack.
