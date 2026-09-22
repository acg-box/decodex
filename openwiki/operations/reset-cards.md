---
type: Reference
title: "Reset Card operation"
description: "Reset Card operation"
tags: ["decodex", "architecture"]
openwiki_generated: true
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-08a47b3cdc5d2b1cdae95c23
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardStore.swift
  - id: openwiki-source-4b6e253ef76717138b4dd66e
    resource: repo://apps/decodex-gpui/src/shell_reset_cards.rs
  - id: openwiki-source-d99870a603f95fac1e865fb2
    resource: repo://crates/decodex-runtime/src/account_launch/api_reset_card.rs
  - id: openwiki-source-b931569075c8af059aefa4d2
    resource: repo://database/src/reset_cards.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

> Current scope: Reset Card redemption is available in Accounts and the explicit CLI, with durable account-scoped recovery. Schema 30 retains the schema-29 operation ledger and adds separate weekly activation. The source/release comparison below is a version-bound implementation receipt, not a statement of the currently installed Codex version. Quota refill animation displays confirmed results; it does not redeem a card. See [Weekly activation](quota-activation.md).

# Reset Card operation

Reset Cards are required Decodex functionality. They are not part of the retired
Managed Repository or GitHub effect layers.

In Accounts, open the account menu, select **Reset Cards**, choose one card, and
confirm **use 1 card**. The panel shows the account and selected card expiry before
confirmation. Cancel sends no request. Refresh only reads inventory and operation
status. The CLI offers the same service through `reset-card list`, `use`, and
`status`; `use` requires an explicit account revision, descriptor, and request key.

## Ownership and safety

The service owns the operation in SQLite migration 29. It holds the account
mutation lock while it validates credentials, revision, and the exact card. It
rejects incomplete inventory, duplicate descriptors, expired cards, and a changed
account or private credit ID. Account health does not depend on an optional Codex
process. No credential refresh or account switch occurs inside a redemption.

The service commits a `sending` barrier before one HTTP POST. Redirects and HTTP
retries are disabled. It passes both the durable request key and the exact private
credit ID to the provider. It never chooses another card when the first fails.

A response with `reset`, `already_redeemed`, `nothing_to_reset`, or `no_credit` is
stored as a receipt. Local completion can resume after a crash without another
provider call. Quota and inventory observations refresh separately. Their failure
does not repeat a successful redemption.

A timeout, lost response, or crash after the send barrier leaves an uncertain
operation. The service does not resend it, and it blocks new redemptions for that
account. Refresh can recover a receipt if the original request finishes later.
An uncertainty that survives service restart stays blocked; do not delete the
ledger or choose another request key to force a retry. This conservative behavior
can require provider-side confirmation before a future recovery feature resolves
it. No automatic recovery spends a card.

The UI can discover the latest operation by account after restart. No UI database,
credential access, or persistent client journal is used. Private credit IDs do not
cross the protocol and are removed from terminal ledger rows. The schema upgrade
adds an empty table and index; it does not alter existing account or credential
rows. Older schema-28 binaries reject schema 29. Downgrade requires the normal
pre-upgrade database backup, not deletion of pending operation evidence.

## Provider contract and validation boundary

The implementation was compared with official `openai/codex` commit
`a2de8fedcc3abe3cdde09b43515db820fb6b95b5`, including backend-client
`rate_limit_resets.rs` and its tests. The installed `codex-cli 0.155.0-alpha.9.2`
experimental app-server schema also exposes reset-credit consumption. Decodex
uses the same backend contract through its existing account API owner so an
explicit account-pool operation does not depend on starting a Codex process.

Tests use fabricated credits, a provider with no HTTP client, and temporary SQLite
roots. They exercise exact selection, duplicate requests, response loss, restart,
receipt recovery, schema upgrade, and UI refusal states. They must not redeem real
credits or open the user's product database. Mock validation does not claim live
redemption acceptance.
