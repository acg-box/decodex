---
type: Reference
title: "Reset Card operation"
description: "Account details disclosure, explicit card confirmation, and durable service-owned redemption and recovery."
tags: [decodex, accounts, operations, reset-cards]
openwiki_generated: true
sources:
  - id: openwiki-source-a21355e56f76651beb4dffc4
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardCLIClient.swift
  - id: openwiki-source-8c181bb99ef43f180f70a6b8
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardPendingAttemptStore.swift
  - id: openwiki-source-51c6a903a86b67bbf46fe288
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardSectionView.swift
  - id: openwiki-source-08a47b3cdc5d2b1cdae95c23
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardStore.swift
  - id: openwiki-source-ec2ac1cc0fd71a4228e71e95
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardUse.swift
  - id: openwiki-source-6bb61549bdedebfcb6463cb5
    resource: repo://apps/decodex-gpui/menubar/Tests/DecodexAppTests/ResetCardPendingAttemptStoreTests.swift
  - id: openwiki-source-2fab31262c7d705356b67f7b
    resource: repo://apps/decodex-gpui/menubar/Tests/DecodexAppTests/ResetCardStoreRecoveryTests.swift
  - id: openwiki-source-4b6e253ef76717138b4dd66e
    resource: repo://apps/decodex-gpui/src/shell_reset_cards.rs
  - id: openwiki-source-1291f5243fa6c9cb52149bda
    resource: repo://apps/decodex-gpui/src/shell.rs
  - id: openwiki-source-bee528a70eef19ac76275c5e
    resource: repo://crates/decodex-app-client-ffi/src/lib.rs
  - id: openwiki-source-be5e68990eacc4bf6ca42685
    resource: repo://crates/decodex-app-client-ffi/src/reset_card_journal.rs
  - id: openwiki-source-6230c010baca677fa60c32c1
    resource: repo://crates/decodex-protocol/src/client.rs
  - id: openwiki-source-d99870a603f95fac1e865fb2
    resource: repo://crates/decodex-runtime/src/account_launch/api_reset_card.rs
  - id: openwiki-source-b931569075c8af059aefa4d2
    resource: repo://database/src/reset_cards.rs
  - id: openwiki-source-e0e48fb115095577a43dbc91
    resource: repo://scripts/macos/test_native_app.sh
generated: { by: "codex", at: "2026-09-30T18:35:20.136Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-10T06:49:11.265Z
---

> Current scope: Reset Card redemption is available in Accounts and the explicit CLI, with durable account-scoped recovery. The embedded schema owns the redemption ledger and separate weekly activation records. The source/release comparison below is a version-bound implementation receipt, not a statement of the currently installed Codex version. Quota refill animation displays confirmed results; it does not redeem a card. See [Weekly activation](quota-activation.md).

# Reset Card operation

Reset Cards are required Decodex functionality. They are not part of the retired
Managed Repository or GitHub effect layers.

## Find and use a card

In Accounts or the native menu, click the account body to open its details. The activity graph and Reset Cards share this disclosure. Multiple accounts can stay open at the same time. A separate drag handle changes account order; account actions and card clicks do not toggle the disclosure.

Each card shows its expiry. Click the selected card once to arm `Confirm · 5s`, then click the same card again within five seconds to use it. Confirmation expires without dispatch. The UI checks the exact account, revision, descriptor and current eligibility again before use. A confirmed result can animate the quota fill; the animation itself has no redemption authority.

There is no separate Reset Cards expand button or manual refresh button in the compact account details. Inventory and operation-state reads remain service-backed. The CLI offers `reset-card list`, `use`, and `status`; `use` requires an explicit account revision, descriptor, and request key.

Account warnings use a severity-colored exclamation icon. Click it to read the explanation; click outside to close it. The popover has no Dismiss button, and closing it does not clear an unresolved operation. Detailed feedback stays near its account. See [account presentation](accounts-and-routing.md#account-rows-and-details) and [menu focus and motion](../architecture/desktop-workspace.md#menu-focus-and-account-motion).

## Ownership and safety

The service owns the operation in SQLite. It holds the account
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

The UI restores account-scoped operation status through the service. The Rust native-client library also keeps a bounded owner-private pending-attempt journal so an unconfirmed request can be reconciled after restart. Swift selects the application-support path and calls the versioned native journal API. That journal does not own provider completion or authorize another redemption. Private credit IDs stay inside the service and are removed from terminal ledger rows.

The Rust protocol client validates the selected account, descriptor, revision, request key and receipt before it reports consumption. The native `consume_reset_card` operation returns the validated operation state or an explicit rejection/possible-dispatch failure. Swift presents that result; it does not decode service command receipts a second time. A possible dispatch preserves the original pending request for status reads. The old `use_reset_card` native operation is not accepted, so a mixed native library cannot interpret the changed response contract.

The journal preserves `reset-card-pending-v1.json` and schema `decodex/reset-card-pending/2`. Rust owns validation, duplicate detection, recovery of valid entries, the 64-attempt limit, process and file locks, and synchronized atomic replacement. Unknown schemas or conflicting records block mutation without deleting the file. A native dispatch lease holds the same file lock across the async request. The `decodex_reset_card_journal_v2` API accepts an observed state, not a removal flag. Rust permits retirement only for completed, failed-before-effect or rejected observations; prepared, ambiguous, missing, unavailable and unconfirmed results retain the exact saved attempt. Swift receives the native retained, removed or removal-failed result. A retained or failed result does not create a new key or send another request. Service recovery cannot replace this journal because a request can lose acknowledgment before the service has a readable operation.

Run `scripts/macos/test_native_app.sh --filter 'ResetCardPendingAttemptStoreTests|ResetCardStoreRecoveryTests'` to exercise the actual Rust journal through Swift. Rust tests also check the legacy document and exclusion of another process while a dispatch lease is held.

Use the current embedded migration ledger for upgrade compatibility. Preserve pending operations and database backups; never clear evidence to force another attempt.

## Provider contract and validation boundary

The implementation was compared with official `openai/codex` commit
`a2de8fedcc3abe3cdde09b43515db820fb6b95b5`, including backend-client
`rate_limit_resets.rs` and its tests. The historical `codex-cli 0.155.0-alpha.9.2`
experimental app-server schema also exposes reset-credit consumption. Decodex
uses the same backend contract through its existing account API owner so an
explicit account-pool operation does not depend on starting a Codex process.

Tests use fabricated credits, a provider with no HTTP client, and temporary SQLite
roots. They exercise exact selection, duplicate requests, response loss, restart,
receipt recovery, schema upgrade, and UI refusal states. They must not redeem real
credits or open the user's product database. Mock validation does not claim live
redemption acceptance.
