---
type: Reference
title: "Historical Program compatibility boundary"
description: "Historical Program compatibility boundary"
tags: ["decodex", "architecture"]
openwiki_generated: true
sources:
  - id: openwiki-source-acd129fd3a88516b785fc929
    resource: repo://crates/decodex-runtime/src/agent/instructions.md
  - id: openwiki-source-dd24c2ff3c2515a21892e312
    resource: repo://database/src/program_cycles.rs
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-10T06:49:11.265Z
---


> Current compatibility boundary: `database/src/program_cycles.rs` provides read-only historical Program aggregates and lineage. This page does not restore a Program execution or write pipeline. See [Agent coordination](../architecture/chief-coordination.md).

# Current implementation boundary

The former Program execution design is retired. Its stored data remains readable; current Agent coordination is a separate execution owner.

The active desktop now centers on Agent and native conversation threads. `agent/instructions.md` permits general-purpose work and recursive coordination. The agent tree and Agent dependency graph replace the old Factory presentation. Program persistence and built-in Domain Pack projections still exist in `database/src/program_cycles.rs` and `crates/decodex-runtime/src/domain_packs.rs`; their presence does not imply a current Factory tab or public extension SDK.

The service entrypoint is `decodex serve`. Repository/GitHub effect orchestration has been retired. A Program proposal or retained historical command never grants fresh effect authority.

Use [Agent coordination](../architecture/chief-coordination.md), [Desktop workspace](../architecture/desktop-workspace.md), and [Runtime architecture](../architecture/runtime-architecture.md) for current behavior. The [August design record](https://github.com/acg-box/decodex/blob/426e862a5978620ffad65fb3d0189cfa57392873/openwiki/decisions/adaptive-program-extension-architecture.md) remains available in Git history.
