---
type: Reference
title: "Service-owned account login"
description: "Service-owned account login"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-c740d34a4e6c4e581873e50e
    resource: repo://crates/decodex-account-login/src/lib.rs
  - id: openwiki-source-6230c010baca677fa60c32c1
    resource: repo://crates/decodex-protocol/src/client.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-f803d54b7400ecfa8c7f5247
    resource: repo://crates/decodex-runtime/src/account_login.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Service-owned account login

## Responsibility

The singleton `AccountLoginManager` in the service coordinates provider authorization and installation through AccountService. The private provider engine is `decodex-account-login`; the UI does not implement OAuth, read auth files, or install credentials.

## Transient protocol

The dedicated same-UID exchange accepts Start, Status and Cancel using an ephemeral canonical session identity. A repeated matching request is idempotent; a different active request is busy. Status carries bounded prompts and completion/failure state in memory, not durable product snapshots or history.

Current clients negotiate exact protocol 2.43. Old 2.11/cohort-7 instructions are obsolete. Native bundle compatibility is checked separately; do not invent a compatibility fallback.

## Installation and cancellation

Provider authorization alone is not successful enrollment. AccountService installs the validated result under exact operation, account and revision fences. Enrollment can restore an existing tombstoned identity; reauthentication targets the exact existing account. Unknown installation outcome stays unknown until authoritative recovery.

Provider work runs outside the async request task. Cancel, replacement and shutdown join the worker and preserve cleanup ordering. The temporary login home verifies its filesystem identity before removal. A cleanup failure must remain visible; it is not permission to abandon credentials or begin an overlapping flow.

## Client and tests

GPUI's `AccountLoginController` uses `AccountLoginClient`. It opens the returned authorization URL or presents a device code, and retains the session identity needed for subsequent status/cancel requests. Normal account projections are credential-negative.

```sh
python3 -m unittest tests/scripts/test_account_login_architecture.py
cargo +stable test -p decodex-protocol account_login
cargo +stable test -p decodex-runtime account_login
```

Keep authorization URLs, device codes, tokens and raw auth documents out of durable examples. Use [Account lifecycle](account-lifecycle-authority.md) for refresh and Route, and [Runtime architecture](../architecture/runtime-architecture.md) for process ownership.
