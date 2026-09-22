---
type: Reference
title: "Weekly quota activation"
description: "Weekly quota activation"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-b52eea0658a5f27f944ae338
    resource: repo://crates/decodex-runtime/src/account_api/activation.rs
  - id: openwiki-source-9b561c5dd3054cdff0599fb9
    resource: repo://database/src/quota_activation.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Weekly quota activation

## Purpose and trigger

A weekly provider reset can expire without advancing until the account makes a request. The service can send one small background Responses request to activate that window. This is separate from redeeming a Reset Card.

The trigger is an expired seven-day reset timestamp that has not advanced, not a rounded percentage. The account must remain enabled at the observed revision and the `auto_activate_quota` preference must be enabled. Other known quota or observation failures can block sending.

## Ownership and outcome

`AccountApiRuntime::observe_and_activate` reuses the existing credential lock and refresh owner. It creates no Codex process or auth file. SQLite reserves the account/reset attempt before the network effect. Successful and ambiguous attempts remain suppressed until the provider advances the reset. Rejections have bounded backoff; absence of a receipt is not permission to retry an ambiguous send.

The request is intentionally minimal and non-persistent. It uses subscription quota but stores no prompt/response conversation. After the attempt, the service observes provider limits again; it does not invent a new reset time or infer success from UI animation.

## Controls and tests

General settings exposes the opt-out; settings changes remain revision-guarded service state. Migration 0030 owns the durable reservation. Tests in `database/src/quota_activation.rs` cover reset progression, duplicate suppression and preference fences. Runtime tests cover the minimal request and outcome classification without spending a real account's quota.

See [Reset Cards](reset-cards.md) and [Local database operations](local-database.md).
