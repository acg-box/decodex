---
type: "Evidence"
title: "SQLite Local-Product Evidence"
description: "Current automated and native evidence for daemon-owned SQLite, protocol-only clients, and the single Decodex.app desktop architecture."
tags: [local-product, sqlite, protocol, gpui, macos, evidence]
openwiki:
  roles: [testing, architecture, workflow]
  change_kinds: [lifecycle, public-api, validation, desktop, packaging]
  source_paths: [database/src/lib.rs, database/src/desktop_settings.rs, database/migrations/0011_desktop_settings.sql, crates/decodex-runtime/src/application.rs, crates/decodex-protocol/src/wire.rs, apps/decodex-gpui/src/client_lifecycle.rs, apps/decodex-gpui/src/settings_surface.rs, apps/decodex-gpui/src/bundled_daemon.rs, scripts/macos/stage_decodex_app.sh, scripts/macos/test_decodex_app_stage.sh]
  test_paths: [database/src/desktop_settings.rs, tests/scripts/test_vnext_architecture.py, tests/scripts/test_account_login_architecture.py, apps/decodex-gpui/src/accounts.rs, apps/decodex-gpui/src/account_profile.rs, apps/decodex-gpui/src/desktop_settings.rs, apps/decodex-gpui/src/shell.rs, scripts/macos/test_decodex_app_stage.sh]
  invariants: [decodexd is the only normal SQLite owner.; GPUI and CLI are protocol-only clients.; Decodex.app is the only macOS GUI bundle.; The optional menu-bar item runs in the GPUI process.; Persistent desktop settings are revision-guarded in SQLite.; Reset Card consumption has no GUI claim without daemon-owned restart discovery.]
  validation_commands: [python3 scripts/vnext/local_database_gate.py, python3 -m unittest tests/scripts/test_vnext_architecture.py tests/scripts/test_account_login_architecture.py, cargo +stable test -p decodex-protocol --all-targets, DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer cargo +stable test -p decodex-gpui --all-targets --features visual-capture, scripts/macos/test_decodex_app_stage.sh]
---

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
- only `apps/decodex-gpui` owns the active GUI source; and
- staging contains no nested login item, helper UI, or client framework.

The one-shot database transfer tool remains separate. Radar and Publisher remain
independent auxiliary CLIs.

## Protocol and GPUI evidence

Protocol 2.10 with artifact cohort 6 adds one desktop-settings query, command, result,
and event. The command requires the current positive revision. `decodexd` commits the
SQLite change and publishes one complete `DesktopSettingsDto`. GPUI's retained-session
controller routes only exact query, receipt, result, and event identities before the
Settings presentation changes `NSStatusItem` visibility.

Focused GPUI tests prove:

- account enrollment and login refresh create only daemon-owned account-login requests;
- enable/disable, fixed/balanced Route, reorder, and logout use revision-guarded protocol
  commands;
- quota rows render only current provider observations;
- account profile is one exact selected-account query;
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
locked or inactive console.

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
