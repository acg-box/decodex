---
type: Reference
title: "Account routing and recovery"
description: "Service-owned account routing and recovery, compact account rows, quota alignment, and cached activity status."
tags: [decodex, accounts, operations, presentation]
verified:
  - by: openwiki/0.6.1
    at: 2026-09-30T17:38:04.493Z
sources:
  - id: openwiki-source-75aea95b5b7fd328b3b6a396
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountProfileViews.swift
  - id: openwiki-source-51c6a903a86b67bbf46fe288
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardSectionView.swift
  - id: openwiki-source-3731a3f9aaef59e0a19b9956
    resource: repo://apps/decodex-gpui/src/account_profile.rs
  - id: openwiki-source-9d711254570d577c97c88bbf
    resource: repo://apps/decodex-gpui/src/quota_meter.rs
  - id: openwiki-source-2dfc8cb7dbdb75c07469c4c3
    resource: repo://apps/decodex-gpui/src/shell_account_activity.rs
  - id: openwiki-source-1291f5243fa6c9cb52149bda
    resource: repo://apps/decodex-gpui/src/shell.rs
  - id: openwiki-source-b52eea0658a5f27f944ae338
    resource: repo://crates/decodex-runtime/src/account_api/activation.rs
  - id: openwiki-source-7415e92f61bcf0ca4195142c
    resource: repo://crates/decodex-runtime/src/account_import.rs
  - id: openwiki-source-e349a925beef4eced1a93943
    resource: repo://crates/decodex-runtime/src/account_launch/macos_attested_spawn.rs
  - id: openwiki-source-b2869e6da778a74cb6afc667
    resource: repo://crates/decodex-runtime/src/account_launch/process.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-e2f4e298ab0a4c683b92158d
    resource: repo://crates/decodex-runtime/src/account_service/personal_access_token.rs
  - id: openwiki-source-e32adebfd6d3bf27dc186bad
    resource: repo://crates/decodex-runtime/src/agent/tests/auth_recovery.rs
  - id: openwiki-source-9b561c5dd3054cdff0599fb9
    resource: repo://database/src/quota_activation.rs
generated: { by: "codex", at: "2026-09-30T17:38:04.493Z" }
---

# Account routing and recovery

## Service ownership

`AccountService` owns account credentials, route selection and refresh coordination. Clients submit typed commands and receive credential-free results. Current Route is a synchronous service transaction: it holds route/account locks, checks revisions and native liveness, refreshes when needed, compares the exact shared-auth source, reads back and records authoritative completion or refusal.

Same-account refresh converges on one token lineage. If native Codex rotates first, Decodex can adopt the valid non-older same-account winner instead of writing back a losing token or making another provider refresh. A live unrelated credential owner or changed source can reject a route. Stored observations are not authorization to redirect unrelated account APIs.

## Imported ChatGPT personal access tokens

Decodex accepts native ChatGPT personal access token (PAT) credentials through the existing Codex-account import flow or an explicitly selected private credential file. The file format uses `decodex/account-credential-import/2`, provider `chatgpt`, and `personal_access_token`. Do not put a token in a command argument or a documentation example. Plugin setup still belongs to Codex for each account.

The account service obtains the account ID, user ID and plan from the fixed native ChatGPT `whoami` authority. It does not infer these values from an opaque token or accept mixed API-key, OAuth, Bedrock or agent-identity fields. FedRAMP metadata is not supported by this path. PAT records have no invented OAuth refresh token or access-token expiry.

For a retained account process, the exact selected credential is supplied through its private `CODEX_ACCESS_TOKEN` environment. Native `getAuthStatus` must confirm `personalAccessToken` with token output disabled. PAT startup does not use the OAuth JWT login or refresh callback. An explicit account refresh rechecks PAT identity; it does not rotate an OAuth refresh token.

Route retains the same native-process quiescence, exact shared-auth comparison and readback rules. It can recognize a previously imported PAT from the local exact credential binding without a network lookup on every observation. An unknown PAT in shared native auth must be imported explicitly before routing. If a token is replaced outside Decodex, import the replacement rather than assuming that an old stored account binding has changed.

## Existing policies

Ordinary model requests and quota activation respect the reviewed native account and workspace/provider policy. Automatic quota activation remains supported. It reserves the observed expired weekly window before its minimal non-persistent request; an ambiguous or completed attempt is not blindly repeated against the same reset window. Reset Card redemption is a separate explicit action with its own durable result.

Account recovery notices and explicit recovery actions remain supported. A missing peak statistic stays unknown rather than becoming zero. Routing failures should expose useful credential-free reasons; debug output must not include secrets.

## Account rows and details

Swift and GPUI use the same interaction model. Normal accounts retain route, power and logout actions. An account that requires a new login shows a red warning with sign-in and logout controls; it does not reserve usage-bar space or require the user to sign in before logging out. Separate drag handles distinguish reordering from a click. Clicking the account body toggles its details; action buttons keep their own behavior.

Each account has independent expanded state. Opening one account does not close another. Details contain the saved activity metrics, usage graph and Reset Cards. Do not add a second Reset Cards disclosure or duplicate logout control. The GPUI account hover covers the row. Native menu focus and synchronized expansion belong to the [desktop presentation boundary](../architecture/desktop-workspace.md).

GPUI keeps `5h` and `7d` quota slots in one row. Labels, percentages and local reset timestamps have stable columns, and a separator distinguishes the two windows. Progress tracks fill the remaining width. An unavailable window keeps its slot with `—` or `N/A`; it does not become a fake zero-percent value. Both clients show the local reset date and time.

## Saved activity status

Cached activity remains readable after refresh fails. Show at most one inline status line: `Sign in again · Saved data` for a required login, or `Saved data` for other cached observations. Use red for the login message and amber/orange for the recoverable warning. Keep detailed native diagnostic text and contextual GPUI explanations in hover surfaces. Loading and empty states use `Loading activity…` and `No activity`.

The short message does not change authentication authority. A rejected or ambiguous refresh, unauthorized access, or rejection after refresh requires another login. A busy or temporarily unavailable credential does not by itself establish that the user must log in again. The native row's existing login-recovery state also controls its warning and action group.

For presentation validation, run the native account presentation and lifecycle tests and the GPUI account disclosure render tests. Check a normal account, an unavailable quota window, a cached provider failure, and a rejected refresh. Inspect the signed app to verify one-line height, matching icon/message severity colors, first-click focus and independent expanded accounts.

## Retired provider recovery recording

The user removed new AWS/Bedrock provider authentication recovery history recording. `modelProvider/authRecoveryStarted` and `modelProvider/authRecoveryCompleted` no longer create local provider recovery receipts. Existing `auth_recovery` history remains readable after restart. This does not disable native authentication, native retry authority, general login diagnostics or account selection.

The retirement regression checks ignored new notifications, no outgoing work and readable pre-retirement history. It is not a live AWS login test.

## Navigation and evidence

See [account lifecycle authority](../specs/account-lifecycle-authority.md), [login authority](../specs/account-login-authority.md), [weekly activation](quota-activation.md), [Reset Cards](reset-cards.md), and [local database operations](local-database.md).
