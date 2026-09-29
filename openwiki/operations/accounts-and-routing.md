---
type: Reference
tags: [decodex, architecture]
title: "Account routing and recovery"
description: "Service-owned routing, credential convergence, quota activation and retired provider recovery recording."
sources:
  - id: openwiki-source-b52eea0658a5f27f944ae338
    resource: repo://crates/decodex-runtime/src/account_api/activation.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-e32adebfd6d3bf27dc186bad
    resource: repo://crates/decodex-runtime/src/agent/tests/auth_recovery.rs
  - id: openwiki-source-9b561c5dd3054cdff0599fb9
    resource: repo://database/src/quota_activation.rs
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T06:24:17.023Z
---


# Account routing and recovery

## Service ownership

`AccountService` owns account credentials, route selection and refresh coordination. Clients submit typed commands and receive credential-free results. Current Route is a synchronous service transaction: it holds route/account locks, checks revisions and native liveness, refreshes when needed, compares the exact shared-auth source, reads back and records authoritative completion or refusal.

Same-account refresh converges on one token lineage. If native Codex rotates first, Decodex can adopt the valid non-older same-account winner instead of writing back a losing token or making another provider refresh. A live unrelated credential owner or changed source can reject a route. Stored observations are not authorization to redirect unrelated account APIs.

## Existing policies

Ordinary model requests and quota activation respect the reviewed native account and workspace/provider policy. Automatic quota activation remains supported. It reserves the observed expired weekly window before its minimal non-persistent request; an ambiguous or completed attempt is not blindly repeated against the same reset window. Reset Card redemption is a separate explicit action with its own durable result.

Account recovery notices and explicit recovery actions remain supported. A missing peak statistic stays unknown rather than becoming zero. Routing failures should expose useful credential-free reasons; debug output must not include secrets.

## Retired provider recovery recording

The user removed new AWS/Bedrock provider authentication recovery history recording. `modelProvider/authRecoveryStarted` and `modelProvider/authRecoveryCompleted` no longer create local provider recovery receipts. Existing `auth_recovery` history remains readable after restart. This does not disable native authentication, native retry authority, general login diagnostics or account selection.

The retirement regression checks ignored new notifications, no outgoing work and readable pre-retirement history. It is not a live AWS login test.

## Navigation and evidence

See [account lifecycle authority](../specs/account-lifecycle-authority.md), [login authority](../specs/account-login-authority.md), [weekly activation](quota-activation.md), [Reset Cards](reset-cards.md), and [local database operations](local-database.md).
