---
type: Reference
title: "ProviderAttempt authority"
description: "ProviderAttempt authority"
tags: ["decodex", "architecture"]
openwiki_generated: true
sources:
  - id: openwiki-source-54d282edcc6b29f87710c554
    resource: repo://crates/decodex-runtime/src/provider_attempt_service.rs
generated: { by: "codex", at: "2026-10-26T14:39:55.302Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-26T14:39:55.302Z
---

# ProviderAttempt authority

## Owner and inputs

`ProviderAttemptService` is the runtime writer and positive-only reconciler of provider-turn effect authority. It consumes an accepted continuation plan and one live fenced ProcessGeneration. SQLite persists attempt identity and transitions; core types define legal states.

This owner has no account selector, RuntimeSession constructor, provider request gateway, automatic retry engine or negative-evidence operation. Conversation orchestration supplies the accepted bindings; provider execution remains in the native adapter.

## Dispatch and uncertainty

Fresh dispatch requires exact attempt, consumer, session/thread and process authority. Persisted observation is not a new live dispatch capability. Timeout, lost response, interruption or process death cannot establish that no provider effect happened.

A replacement service can reconcile an original attempt with positive provider evidence but cannot replay it. Exact thread/turn readback and terminal evidence distinguish confirmed completion from unknown outcomes. UI state, graph movement and a finished worker message are not provider receipts.

## Current persistence and verification

The current implementation uses the current SQLite schema. User state and exact provider receipts retain their owners; application startup has no historical upgrade chain.

Focused tests belong to `provider_attempt_service.rs`, the SQLite provider-attempt owner, conversation restart fixtures and native adapter tests. Live acceptance is separate from fabricated provider responses. See [ProcessGeneration](process-generation-authority.md), [Current database contract](../operations/local-database.md), and [Commands and validation](../operations/commands-and-validation.md).
