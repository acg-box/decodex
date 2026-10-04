---
type: Reference
title: "Weekly quota activation"
description: "Weekly quota activation"
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-b52eea0658a5f27f944ae338
    resource: repo://crates/decodex-runtime/src/account_api/activation.rs
  - id: openwiki-source-6c8d2efdbf89ccf02451df41
    resource: repo://crates/decodex-runtime/src/account_launch/activation_policy.rs
  - id: openwiki-source-9b561c5dd3054cdff0599fb9
    resource: repo://database/src/quota_activation.rs
generated: { by: "codex", at: "2026-10-03T17:31:35.485Z" }
verified:
  - by: openwiki/0.7.0
    at: 2026-10-03T17:31:35.485Z
---


# Weekly quota activation

## Purpose and trigger

A weekly provider reset can expire without advancing until the account makes a request. The service can send one small background Responses request to activate that window. This is separate from redeeming a Reset Card.

Activation can follow either an expired seven-day reset timestamp or a floating reset that keeps moving approximately seven days ahead. The store compares samples at least 30 seconds apart, with a 15-second clock tolerance, to identify a floating window. Percentages do not trigger activation. The account must remain enabled at the observed revision and the `auto_activate_quota` preference must be enabled. Other known quota or observation failures can block sending.

## Ownership and outcome

`AccountApiRuntime::observe_and_activate` reuses the existing credential lock and refresh owner. It starts a short-lived, account-bound native process to read the activation policy, then shuts that process down before the direct Responses request. SQLite reserves the account/reset attempt before the network effect. Successful and ambiguous attempts remain suppressed for the reserved window. A floating window stays reserved until a real countdown starts, including after a restart. Rejections have bounded backoff; absence of a receipt is not permission to retry an ambiguous send.

The request is intentionally minimal and non-persistent. It uses subscription quota but stores no prompt/response conversation. After the attempt, the service observes provider limits again; it does not invent a new reset time or infer success from UI animation.

## Controls and tests

General settings exposes the opt-out; settings changes remain revision-guarded service state. The embedded baseline schema owns `account_quota_activation`; use the current migration ledger instead of historical migration numbers. Tests in `database/src/quota_activation.rs` cover reset progression, duplicate suppression and preference fences. Runtime tests cover the minimal request and outcome classification without spending a real account's quota.

See [Reset Cards](reset-cards.md) and [Local database operations](local-database.md).
