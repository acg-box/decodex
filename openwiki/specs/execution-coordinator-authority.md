---
type: Reference
title: "Execution coordination: current owners and historical design"
description: "Current ordinary and Agent coordination owners, with a retained superseded sequencing design."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-ec5c9f32d2135154f4297a49
    resource: repo://crates/decodex-runtime/src/conversation.rs
generated: { by: "codex", at: "2026-10-21T09:01:22.045Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-24T21:11:31.338Z
---

# Current execution boundary

The historical design describes the former server-store/ManagedRun sequencing proposal and its milestone status. It does not define a current executable coordinator or authorize retired repository/GitHub effects.

Current ordinary conversations are coordinated by `crates/decodex-runtime/src/conversation.rs` over the SQLite store, process supervisor, provider-attempt service and native Codex adapter. Agent adds a distinct durable coordination owner in `agent.rs` and `agent_host.rs`. Neither UI graph is a scheduler. A restored record does not mint a new live process fence, and uncertainty is not replay permission.

Use [Runtime architecture](../architecture/runtime-architecture.md), [Agent coordination](../architecture/chief-coordination.md), [ProcessGeneration](process-generation-authority.md), and [ProviderAttempt](provider-attempt-authority.md) for current implementation boundaries. The [original sequencing design](https://github.com/acg-box/decodex/blob/426e862a5978620ffad65fb3d0189cfa57392873/openwiki/specs/execution-coordinator-authority.md) and its dated acceptance statements remain in Git history. Its disposable-data and no-migration directions do not apply to the current product.
