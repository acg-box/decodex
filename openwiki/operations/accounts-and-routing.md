---
type: Reference
title: "Account routing and recovery"
description: "Service-owned account routing and recovery, compact account rows, quota alignment, and cached activity status."
tags: [decodex, accounts, operations, presentation]
sources:
  - id: openwiki-source-96374b161003fb4874b09534
    resource: repo://apps/decodex-cli/src/account.rs
  - id: openwiki-source-230c7c1843682509316cd745
    resource: repo://apps/decodex-cli/tests/account_route_process.rs
  - id: openwiki-source-98e7b23c4cc276d20fcb4649
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountControlViews.swift
  - id: openwiki-source-51c6a903a86b67bbf46fe288
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardSectionView.swift
  - id: openwiki-source-fecafeb22abda7dad3877372
    resource: repo://apps/decodex-gpui/src/account_feedback.rs
  - id: openwiki-source-9d711254570d577c97c88bbf
    resource: repo://apps/decodex-gpui/src/quota_meter.rs
  - id: openwiki-source-2dfc8cb7dbdb75c07469c4c3
    resource: repo://apps/decodex-gpui/src/shell_account_activity.rs
  - id: openwiki-source-4b6e253ef76717138b4dd66e
    resource: repo://apps/decodex-gpui/src/shell_reset_cards.rs
  - id: openwiki-source-1291f5243fa6c9cb52149bda
    resource: repo://apps/decodex-gpui/src/shell.rs
  - id: openwiki-source-fe671b38d0336d4630edcd46
    resource: repo://apps/decodex-gpui/src/ui_disclosure.rs
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
  - id: openwiki-source-ff8abff5a675874c627f1969
    resource: repo://crates/decodex-runtime/src/account_service/tests/plan_scope.rs
  - id: openwiki-source-e32adebfd6d3bf27dc186bad
    resource: repo://crates/decodex-runtime/src/agent/tests/auth_recovery.rs
  - id: openwiki-source-63bea5f3704fcd7e4b161192
    resource: repo://database/src/account_lifecycle.rs
  - id: openwiki-source-9b561c5dd3054cdff0599fb9
    resource: repo://database/src/quota_activation.rs
  - id: openwiki-source-8fe6f1557e5d45d88b34bd0a
    resource: repo://scripts/macos/generate_workspace_symbols.swift
generated: { by: "codex", at: "2026-11-23T03:02:45.946Z" }
verified:
  - by: openwiki/0.9.0
    at: 2026-11-23T03:02:45.946Z
---

# Account routing and recovery

## Service ownership

Account admission is limited to the native plans `plus`, `pro`, `prolite`, `promax` and `team`, with case-insensitive names. Missing, unknown and other plans are rejected. The same admission rule applies to OAuth and PAT credentials. Team is the workspace plan within this scope; the four individual plans share the existing account flow. Admission does not grant a model entitlement or a fixed quota. Native identity, usage and workspace policy remain authoritative.

`AccountService` owns account credentials, route selection and refresh coordination. Clients submit typed commands and receive credential-free results. Current Route is a synchronous service transaction: it holds route/account locks, checks revisions and native liveness, refreshes when needed, compares the exact shared-auth source, reads back and records authoritative completion or refusal.

Same-account refresh converges on one token lineage. If native Codex rotates first, Decodex can adopt the valid non-older same-account winner instead of writing back a losing token or making another provider refresh. A live unrelated credential owner or changed source can reject a route. Stored observations are not authorization to redirect unrelated account APIs.

## Switch away from a downgraded or removed account

Select the account you want to use. If the current Codex account is still in Decodex and its plan has changed, you can switch away from it. Decodex saves a newer valid sign-in for that account. The selected target must qualify for use.

If the shared Codex OAuth sign-in belongs to an account outside Decodex, the desktop and menu bar show the current sign-in and the selected target. Select **Replace sign-in and switch** to replace that local sign-in. Select **Cancel** to keep it. You can select a target that is already fixed if the shared login has changed. If a native app is using the sign-in, quit it and try again. If the sign-in changes before you confirm, select the target again and review the new sign-in.

In the CLI, run `decodex account route --account-id <TARGET> --idempotency-key <KEY>`. If the result is `shared_login_replacement_required`, review `source_label`. Run the same command for the same target with a new idempotency key and `--replace-shared-login <CONFIRMATION>`. Use the `confirmation` value from the result. Other enrolled accounts keep their saved credentials.

## Remove an account after a rejected refresh

Select **Remove account from Decodex** to remove the account and its saved sign-in. You can remove an account after a rejected refresh without signing in again. The service checks the account revision and credential binding, marks that refresh failure as superseded and keeps its original reason. A repeat of the same removal command returns its saved result.

A pending operation or an ambiguous provider result blocks removal. Resolve that operation through its existing recovery flow. The shared Codex sign-in stays in place until you switch accounts.

## Imported ChatGPT personal access tokens

Decodex accepts native ChatGPT personal access token (PAT) credentials through the existing Codex-account import flow or an explicitly selected private credential file. The file format uses `decodex/account-credential-import/2`, provider `chatgpt`, and `personal_access_token`. Do not put a token in a command argument or a documentation example. Plugin setup still belongs to Codex for each account.

The account service obtains the account ID, user ID and plan from the fixed native ChatGPT `whoami` authority. It does not infer these values from an opaque token or accept mixed API-key, OAuth, Bedrock or agent-identity fields. FedRAMP metadata is not supported by this path. PAT records have no invented OAuth refresh token or access-token expiry.

For a retained account process, the exact selected credential is supplied through its private `CODEX_ACCESS_TOKEN` environment. Native `getAuthStatus` must confirm `personalAccessToken` with token output disabled. PAT startup does not use the OAuth JWT login or refresh callback. An explicit account refresh rechecks PAT identity; it does not rotate an OAuth refresh token.

Route retains the same native-process quiescence, exact shared-auth comparison and readback rules. It can recognize a previously imported PAT from the local exact credential binding without a network lookup on every observation. An unknown PAT in shared native auth must be imported explicitly before routing. If a token is replaced outside Decodex, import the replacement rather than assuming that an old stored account binding has changed.

## Existing policies

Ordinary model requests and quota activation respect the reviewed native account and workspace/provider policy. Automatic quota activation remains supported. It reserves the observed expired weekly window before its minimal non-persistent request; an ambiguous or completed attempt is not blindly repeated against the same reset window. Reset Card redemption is a separate explicit action with its own durable result.

Account recovery notices and explicit recovery actions remain supported. A missing peak statistic stays unknown rather than becoming zero. Routing failures should expose useful credential-free reasons; debug output must not include secrets.

## Account rows and details

Swift and GPUI show route, power and removal actions on normal accounts. When an account requires a new login, its row shows a red warning and actions to sign in or remove the account. The row uses the space normally reserved for usage bars. Drag the handle to reorder accounts or click the summary to toggle details. In GPUI, the hand cursor belongs to that summary. Expanded metrics and the chart use the normal cursor, and clicking them keeps the account open.

Each account has independent expanded state. Opening one account does not close another. Details contain the saved activity metrics, usage graph and Reset Cards. GPUI uses the shared 200 ms disclosure animation for expansion and collapse. Each moving frame measures current content before placing the next account and clips the visible area. It does not reuse a previous frame's content height. Already open details use their full height on first entry. Do not add a second Reset Cards disclosure or duplicate logout control. The GPUI account hover covers the row. Native menu focus and synchronized expansion belong to the [desktop presentation boundary](../architecture/desktop-workspace.md).

GPUI places the compact divided list in one shared Settings panel. The header uses a primary Add account action. Each summary aligns identity, short `5h` and `7d` quota meters and management actions on one row. The quota column has a fixed width, and one or two windows keep the same summary height. Labels, percentages and local reset timestamps retain stable columns. A not-applicable window has no row, matching Menu Bar. Unknown usage retains its row with a placeholder; it does not become a fake zero-percent value. Both clients show the local reset date and time and pad single-digit calendar days with a digit-width space. On macOS, Foundation supplies the system hour-cycle preference, locale and time zone for quota reset and Reset Card expiry labels. Quoted locale words stay literal. This changes presentation only; it does not change the native reset instant or quota evidence.

In GPUI, select **Choose automatically** to use automatic account selection for new conversations. **Automatic** shows the active mode. Select an account's route action to use a fixed account. The service applies the route. During sign-in, the toolbar shows **Cancel**. When a link or device code becomes available, the toolbar also shows **Copy link** or **Copy code** and **Open sign-in page**. These actions fit within the minimum window width.

The route action uses a curved connection between two endpoints in Settings and Menu Bar. GPUI highlights the fixed target and lets you select it again to restore its shared sign-in. The branching-arrow icon is reserved for conversation Fork actions.

Expanded activity uses a two-column grid of labels and values. Total tokens, Daily peak, Streak and Longest task remain separate metrics. Streak combines the current and best values. Labels and values share the quota text size and muted color. Values use tabular figures without extra size or weight. The daily chart uses a subdued accent, and Reset cards has its own divided area. Missing metrics stay absent. Activity remains bound to the current account revision.

## Saved activity status

Cached activity remains readable after refresh fails. GPUI shows shared inline feedback beside the affected account, with a severity icon and readable explanation. The native menu-bar keeps a compact feedback icon and a click-open explanation. Clicking outside closes that native popover without clearing the underlying warning.

This pattern also covers account availability and lifecycle warnings, plus the orange warning to quit ChatGPT or Codex before switching accounts. GPUI places the explanation with its owner; the native menu-bar keeps it in the compact feedback control. Native usage diagnostics appear in the account explanation. A loading indicator can remain while activity is being fetched. Actual login, logout and route actions retain their own controls.

The explanation does not change authentication authority. A rejected or ambiguous refresh, unauthorized access, or rejection after refresh requires another login. A busy or temporarily unavailable credential does not by itself establish that the user must log in again. The native row's existing login-recovery state also controls its warning and action group.

For presentation validation, run the native account presentation and lifecycle tests and the GPUI account disclosure render tests. Check a normal account, an unavailable quota window, a cached provider failure, and a rejected refresh. Inspect the signed app to verify account layout, GPUI inline explanations, native popover closure, first-click focus and independent expanded accounts.

## Retired provider recovery recording

The user removed new AWS/Bedrock provider authentication recovery history recording. `modelProvider/authRecoveryStarted` and `modelProvider/authRecoveryCompleted` no longer create local provider recovery receipts. Existing `auth_recovery` history remains readable after restart. This does not disable native authentication, native retry authority, general login diagnostics or account selection.

The retirement regression checks ignored new notifications, no outgoing work and readable pre-retirement history. It is not a live AWS login test.

## Navigation and evidence

See [account lifecycle authority](../specs/account-lifecycle-authority.md), [login authority](../specs/account-login-authority.md), [weekly activation](quota-activation.md), [Reset Cards](reset-cards.md), and [local database operations](local-database.md).
