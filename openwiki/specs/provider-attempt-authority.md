---
type: Reference
title: "ProviderAttempt authority"
description: "ProviderAttempt authority"
tags: ["decodex", "architecture"]
openwiki_generated: true
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-54d282edcc6b29f87710c554
    resource: repo://crates/decodex-runtime/src/provider_attempt_service.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# ProviderAttempt authority

## Owner and inputs

`ProviderAttemptService` is the runtime writer and positive-only reconciler of provider-turn effect authority. It consumes an accepted continuation plan and one live fenced ProcessGeneration. SQLite persists attempt identity and transitions; core types define legal states.

This owner has no account selector, RuntimeSession constructor, provider request gateway, automatic retry engine or negative-evidence operation. Conversation orchestration supplies the accepted bindings; provider execution remains in the native adapter.

## Dispatch and uncertainty

Fresh dispatch requires exact attempt, consumer, session/thread and process authority. Persisted observation is not a new live dispatch capability. Timeout, lost response, interruption or process death cannot establish that no provider effect happened.

A replacement service can reconcile an original attempt with positive provider evidence but cannot replay it. Exact thread/turn readback and terminal evidence distinguish confirmed completion from unknown outcomes. UI state, graph movement and a finished worker message are not provider receipts.

## Current persistence and verification

The current implementation uses SQLite rather than the historical server-store function/ACL scheme. Existing user state and migration history must be preserved.

Focused tests belong to `provider_attempt_service.rs`, the SQLite provider-attempt owner, conversation restart fixtures and native adapter tests. Live acceptance is separate from fabricated provider responses. See [ProcessGeneration](process-generation-authority.md), [Current product contract](local-product-v1.md), and [Commands and validation](../operations/commands-and-validation.md).
