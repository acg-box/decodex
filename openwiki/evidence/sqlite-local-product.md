---
type: Reference
title: "SQLite local-product evidence and current verification boundary"
description: "SQLite local-product evidence and current verification boundary"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:55:18.668Z
sources:
  - id: openwiki-source-acf49c93c3e80379f0023c71
    resource: repo://apps/decodex-gpui/src/accounts.rs
  - id: openwiki-source-cc0439b23243c3697ba49199
    resource: repo://crates/decodex-protocol/src/lib.rs
  - id: openwiki-source-f4724776aade804ebf838e2e
    resource: repo://crates/decodex-runtime/src/account_service.rs
  - id: openwiki-source-601aed9bf7f72a4b5d4a6e78
    resource: repo://database/src/migrations.rs
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-09-22T05:55:18.668Z" }
---

# Current verification boundary

The detailed receipt below describes the older desktop consolidation, not the current build. Its protocol 2.11, cohort 7, schema 11, decodexd helper, Factory UI, and statement that Reset Card GUI consumption is absent are superseded.

The current local protocol is 2.43. SQLite schema 30 includes durable Reset Card operations and quota activation. The package contains `Contents/Helpers/decodex`; Accounts and CLI now expose service-owned Reset Card operations with account-scoped recovery. `scripts/macos/test_decodex_app_stage.sh` verifies bundle identity, signatures and native ABI, including a negative mismatch fixture. These source-defined checks are not a fresh live acceptance receipt from this documentation run.

Current Route is synchronous and credential-negative. The service returns authoritative completion or refusal; the historical Pending Route process-list DTO is no longer present. Desktop clients bind the result to their active session and exact command. Existing Claim IDs are updated to those current owners.

See [Runtime architecture](../architecture/runtime-architecture.md), [Reset Cards](../operations/reset-cards.md), and [Commands and validation](../operations/commands-and-validation.md).

---

## Preserved consolidation receipt

> Historical acceptance record: the Reset Card GUI retirement below is superseded
> by [Reset Card operation](../operations/reset-cards.md). This record does not
> describe current Reset Card support or authorize card-consuming acceptance tests.

# SQLite Local-Product Evidence

Status: current implementation and validation evidence.

Date: 2026-08-24.

## Product-state and process boundary

The fresh local database gate initialized schema version 11 twice, validated it through
`decodexd`, and read it back in WAL mode with `quick_check`, foreign-key verification,
all eleven exact migration digests, and a 41-table inventory. The new
`desktop_settings` singleton defaults to a visible menu-bar item and carries one positive
revision. A focused database test changed it, rejected a stale revision, reopened the
database, and read the changed value back. A separate exact schema-10 fixture upgraded
through migration 11, received the default setting at revision 1, and preserved its
pre-existing account identity and task profile.

Static architecture tests prove:

- only `decodex-runtime` depends on `decodex-database`;
- GPUI and CLI do not depend on SQLite, redb, or the database crate;
- the retired Control application, Swift companion, native client bridge, and GPUI spike
  are absent;
- only `apps/decodex-gpui` owns the active GUI source;
- login-item startup orders out native windows instead of presenting the main window; and
- staging contains no nested login item, helper UI, or client framework.

The one-shot database transfer tool remains separate. Radar and Publisher remain
independent auxiliary CLIs.

`tests/scripts/test_vnext_architecture.py` statically checks the login-item branch for
`order_out_native_windows()` and `window.orderOut(None)`, protecting the quiet-startup
contract without requiring a live macOS window session.

## Protocol and GPUI evidence

Protocol 2.11 with artifact cohort 7 keeps the desktop-settings query, command, result,
and event and adds one closed, credential-negative `AccountRouteWaitReason` to every Pending
Route. Concrete external blockers carry only a bounded PID, `ChatGPT` or `Codex` identity,
and shared-versus-unknown auth-home evidence. Other variants name process-observation,
account-readiness, source-stability, source-availability, or projection-readback waits. The
cohort fence prevents older daemon, native bridge, and GUI artifacts from accepting this new
exact wire shape.

The desktop-setting command still requires the current positive revision. `decodexd` commits
the SQLite change and publishes one complete `DesktopSettingsDto`. GPUI's retained-session
controller routes only exact query, receipt, result, and event identities before the Settings
presentation changes `NSStatusItem` visibility.

Focused GPUI tests prove:

- account enrollment and login refresh create only daemon-owned account-login requests;
- enable/disable, fixed/balanced Route, reorder, and logout use revision-guarded protocol
  commands;
- quota rows render only current provider observations;
- account profile is one exact selected-account query;
- Pending Route results reject malformed blocker/readiness shapes and render the current PID or
  exact typed fallback reason in both desktop surfaces;
- desktop-setting commands accept only the matching daemon result; and
- the simulated menu-bar host owns one in-process item.

The complete GPUI run passed 136 main tests, 19 Factory visual-capture tests, and 136
Workbench visual-capture tests. Three live-daemon Quick Task tests are intentionally
ignored in each shell-bearing binary; they create user product state and are unrelated to
desktop consolidation.

## Bundle and native runtime evidence

The current release staging test builds and signs one `Decodex.app` with identifier
`box.acg.decodex` and executable `decodex-gpui`. The bundle contains the signed
`Contents/Helpers/decodexd`, `Contents/Frameworks/libDecodexMenuBar.dylib`, and
`Contents/Frameworks/libdecodex_app_client_ffi.dylib` payloads. It proves there is exactly
one `.app` under the stage root, one helper, two framework files, matching signing teams,
and no `Contents/Library/LoginItems` directory. This preserves one GUI process and one
product authority while allowing local profiles to launch the embedded daemon.

Before the final native activation repair, the accessibility gate launched the staged
bundle and passed:

- one matching application PID and executable identity;
- one `Decodex` window;
- current Workbench, Factory, Accounts, Health, and Settings accessibility roles;
- forward and reverse keyboard focus;
- selection activation; and
- screenshot pixel and bundle-fingerprint checks.

That earlier receipt reported `passed: true` and was inspected before its isolated
runtime-evidence directory was removed. The later source change replaced GPUI's
deprecated macOS activation path with main-thread `NSApplication.activate()` and made
the inspector retry activation, require positive active readback, and fail early for a
locked or inactive console. A subsequent quiet-login repair makes the login-item branch call
`order_out_native_windows`, which orders out native windows created during background startup;
ordinary reopen continues through `activate_main_window`.

The cold gate for the exact final binary cannot complete in the current environment.
`CGSessionCopyCurrentDictionary` reports `kCGSSessionOnConsoleKey=1` and
`CGSSessionScreenIsLocked=1`. The harness verified the exact staged bundle launch
identity and terminated it without a survivor, then the inspector stopped before any
Accessibility-tree or screenshot assertion. Therefore, the earlier passing receipt is
pre-activation-repair evidence; it is not current exact-source visual acceptance.

An isolated schema-11 daemon and the pre-activation-repair staged application provided
live desktop acceptance. Computer Use read the online Settings surface, changed **Show
Decodex in the menu bar** off and on, and read both authoritative states. The visible
status menu contained `Open Decodex`, an account-workflow description, and `Quit Decodex`. Process
readback showed one GPUI application PID with no child UI process and no retired bundle
identity. After both application and daemon restart, Settings again read `VISIBLE` and
the switch remained on.

## Retired companion workflow classification

The former companion workflows have these current dispositions:

- account login/refresh, enable/disable, Route, reorder, confirmed logout, quota, and
  profile: implemented in GPUI over the daemon protocol;
- Reset Card service and explicit CLI: retained; and
- Reset Card GUI consumption: intentionally retired until `decodexd` exposes pending
  operation discovery that removes the need for UI-owned persistent recovery state.

No current document or test claims that GPUI supports Reset Card consumption.

## Remaining evidence boundary

The staged application now requires the stable Apple signing identity supplied through
`DECODEX_APP_SIGN_IDENTITY`; ad-hoc signing is rejected by the canonical stage script.
Developer ID distribution, notarization, and installation into `/Applications` were not
part of this repository-writing task. The local native checks prove source-built bundle
shape. A final unlocked cold accessibility run remains required to re-accept exact-source
visual and keyboard behavior after the native activation repair.
