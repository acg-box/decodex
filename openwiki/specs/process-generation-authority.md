---
type: Reference
title: "ProcessGeneration authority"
description: "ProcessGeneration authority"
tags: ["decodex", "architecture"]
openwiki_generated: true
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-96d8b5b0b0f9c7e15da20cda
    resource: repo://crates/decodex-runtime/src/process_supervisor.rs
  - id: openwiki-source-ecc4549853adbfae98185da4
    resource: repo://database/src/chief_process.rs
  - id: openwiki-source-f2137915c6cf8c70697a023f
    resource: repo://database/src/process_generations.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# ProcessGeneration authority

## Responsibility and state

ProcessGeneration is durable authority for a provider process: account binding, attested launch identity, execution epoch, exact process identity, revision and positive death evidence. It does not select accounts, create provider turns, or grant UI effects.

`ProcessSupervisor` owns runtime mutation and positive-only reconciliation. `database/src/process_generations.rs` owns SQLite transitions and fences; `decodex-core` owns typed domain values. The former server-store ACL/function design is superseded by this SQLite implementation.

## Launch and recovery

Fresh launch authority is an opaque, attested account-process capability. The launch owner verifies executable/schema/account bindings before the supervisor receives a fresh fence. It does not accept an arbitrary reconstructed command from persisted state.

Durable process identity and live control authority are different. After restart, processes may be observed but are not automatically adopted, reacquired or signaled. Unknown death remains uncertain and prevents unsafe replacement. Account-local quarantine must not become a global permanent failure.

Prior-boot death is valid only with the correct positive kernel evidence. Chief process bindings use this evidence to release old affinity after reboot; an arbitrary exit claim or mismatched boot identity must not do so.

## Tests and related owners

Tests in `database/src/process_generations.rs`, `database/src/chief_process.rs`, `process_supervisor.rs` and the account-launch modules cover fresh fencing, restart and exact identity. Live spawn acceptance additionally needs the platform signing and process-lifetime conditions.

See [ProviderAttempt](provider-attempt-authority.md), [Chief coordination](../architecture/chief-coordination.md), and [Runtime architecture](../architecture/runtime-architecture.md). A dead process is not proof that its last provider effect did not occur.
