---
type: Reference
title: "Commands and validation"
description: "Repository command authority and source, native and desktop acceptance boundaries."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-95772fa38252ded838ec3a2f
    resource: repo://.config/nextest.toml
  - id: openwiki-source-586227b5591e5f24cdfe6427
    resource: repo://apps/decodex-cli/src/agent.rs
  - id: openwiki-source-2e9ba9281afc8bacff8a0775
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/DecodexNativeClient.swift
  - id: openwiki-source-90188458ff8e2e5499bd7ca8
    resource: repo://apps/decodex-gpui/menubar/Tests/DecodexAppTests/NativeJournalFixture.swift
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-2d990e4e2bc233a95b4894b9
    resource: repo://crates/decodex-components/src/pin.rs
  - id: openwiki-source-95a7168e447de14b4b758f16
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_archive_tests.rs
  - id: openwiki-source-1a964823f348372d89aa1920
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_code_mode_tests.rs
  - id: openwiki-source-9145039d5c88ff2144ae6a60
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_discovery_tests.rs
  - id: openwiki-source-053611cb863c8c6865cef2ca
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_hook_settings_tests.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-ba12412e93bca80301ac28a3
    resource: repo://crates/decodex-runtime/tests/native_qualification/apps_recovery.py
  - id: openwiki-source-e7a53f398340c29957bd81d3
    resource: repo://crates/decodex-runtime/tests/native_qualification/native_session.py
  - id: openwiki-source-8e6f29de9ed0d4825985ec62
    resource: repo://crates/decodex-runtime/tests/native_qualification/project_guardian.py
  - id: openwiki-source-2fee1dea7ba19d72acce7a3c
    resource: repo://crates/decodex-runtime/tests/native_qualification/proxy_headers.py
  - id: openwiki-source-aac8f76a32012d4a3e5a445d
    resource: repo://crates/decodex-runtime/tests/native_qualification/terminal_controls.py
  - id: openwiki-source-157a50123cd9da439c18985d
    resource: repo://crates/decodex-runtime/tests/native_qualification/usage.py
  - id: openwiki-source-6197a876b2effbe992b1e2ec
    resource: repo://database/src/agent.rs
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
  - id: openwiki-source-b7793decf9d7c9ba48e57e0f
    resource: repo://rust-toolchain.toml
  - id: openwiki-source-8341ffcf9eaf781ab2d51b69
    resource: repo://scripts/macos/compile_decodex_app_icon.sh
  - id: openwiki-source-58e225584250806d8658ea09
    resource: repo://scripts/macos/prepare_codex.mts
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
  - id: openwiki-source-e0e48fb115095577a43dbc91
    resource: repo://scripts/macos/test_native_app.sh
generated: { by: "codex", at: "2026-10-25T03:47:07.956Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T03:47:07.956Z
---


# Commands and validation

Use the smallest check that proves the changed contract. Broaden testing for shared runtime, protocol, database or packaging changes. The repository uses stable Rust for builds and tests; formatting uses the unversioned `nightly` channel selected by `Makefile.toml`. Do not select numbered compiler versions or dated nightly versions. Run `cargo make fmt-rust` to format and `cargo make fmt-rust-check` to check formatting.

## Source and runtime owners

| Owner | Responsibility |
| --- | --- |
| `database/` | SQLite schema, migration ledger, transactions and restart fixtures |
| `decodex-core` | Domain types, bounded identities, paths and pure policies |
| `decodex-codex` | Native app-server transport and adapters |
| `decodex-components` | Runtime requirements, verified acquisition and component cache repair |
| `decodex-runtime` | Service application, account effects, conversations and Agent |
| `decodex-protocol` | Exact typed local wire contract and clients |
| `apps/decodex-cli` | Unified `decodex` commands and `serve` |
| `apps/decodex-gpui` | GPUI app, native Swift libraries and UI tests |
| `apps/radar`, `apps/decodex-publisher` | Independent research/publication tools |
| `site/` | Static Astro product site |

Do not use removed `apps/decodexd`, Factory capture binaries, or old server-store gates.

## Focused checks

```sh
cargo +stable test -p decodex-database --lib
cargo +stable test -p decodex-protocol --lib
cargo +stable test -p decodex-runtime --lib
cargo +stable test -p decodex-codex --lib
cargo +stable test -p decodex-gpui --bin decodex-gpui
cargo +stable test -p decodex-cli --all-targets
python3 scripts/vnext/local_database_gate.py
python3 -m unittest tests/scripts/test_vnext_architecture.py
python3 -m unittest tests/scripts/test_account_login_architecture.py
```

The database gate uses an isolated owner-private root. It is not permission to reset the user's database. Ignored live-provider tests require their documented account and effect prerequisites; do not enable every ignored test as a routine check.

## Discard an unused local draft

Inspect the exact work ID with `decodex --output json agent status`. For a confirmed empty placeholder, use the owning service:

```sh
decodex --output json agent discard-empty --work-id LOCAL_DRAFT_ID --idempotency-key CLEANUP_ID
```

Replace both placeholders with explicit identities. The service refuses a root, a bound native thread, active or scheduled work, descendants, inbox history, dependencies, thread revisions and saved prompt input. The transaction removes only an unused local draft. Repeating cleanup for an absent ID succeeds without another effect. Do not remove live rows with direct SQLite writes.

Archive a real conversation in Codex instead. The Decodex active view follows native archive membership and retains the local history. See [Archive synchronization](../architecture/chief-coordination.md#archive-synchronization).

The focused regression checks are:

```sh
cargo +stable test --locked -p decodex-database discard_empty_work
cargo +stable test --locked -p decodex-runtime --lib agent_visibility
cargo +stable test --locked -p decodex-codex archive_catalog
cargo +stable test --locked -p decodex-gpui --bin decodex-gpui native_list_removes_retired_tabs
```

## Native archive refresh qualification

Use the already available pinned Codex executable for the opt-in cross-process check:

```sh
DECODEX_TEST_CODEX_BINARY=/absolute/path/to/codex cargo +stable nextest run --locked -p decodex-runtime --lib --run-ignored only -E 'test(installed_native_archive_roundtrip)'
```

The test creates a private native home and a loopback Responses fixture. It adds 10,300 unrelated archive records, then runs two archive and unarchive cycles through two retained native processes. It checks exact identity, active descendants, restored hierarchy and the absence of additional model turns. Neither process restarts during those cycles. The command does not install a runtime or use the user's conversation store.

The GPUI `archived_selection_parks_exact_draft_and_restores_it_on_unarchive` check covers the list projection, relationship graph and original draft, including attachments and references. These checks support the source build; they do not prove that an older installed app contains the change. Use a freshly built app for interactive acceptance and keep installation separate.

## Qualify retained native behavior

Use [the portable evidence index](../../automations/upstream/retained-evidence.json) for current retained requirements, exact test names and qualification limits. Use its command and environment with the candidate CLI and its packaged helpers. Register the selected tests first and include ignored native fixtures explicitly. Require every listed test to execute. Keep source tests, local projection tests and actual CLI tests in separate receipts.

The reusable artifact probes are in `crates/decodex-runtime/tests/native_qualification/`. Set `QUALIFIED_CODEX_BINARY` to the absolute candidate CLI path and `QUALIFIED_CODEX_SOURCE_COMMIT` to its exact upstream commit. Set `DECODEX_QUALIFICATION_OUTPUT` to an existing output directory under the native automation run, outside tracked files. Keep the packaged Code Mode helper beside the selected package.

```sh
python3 crates/decodex-runtime/tests/native_qualification/usage.py
python3 crates/decodex-runtime/tests/native_qualification/terminal_controls.py
python3 crates/decodex-runtime/tests/native_qualification/apps_recovery.py
python3 crates/decodex-runtime/tests/native_qualification/proxy_headers.py
python3 crates/decodex-runtime/tests/native_qualification/thread_settings.py
python3 crates/decodex-runtime/tests/native_qualification/log_storage.py
python3 crates/decodex-runtime/tests/native_qualification/model_errors.py
python3 crates/decodex-runtime/tests/native_qualification/large_catalog.py
python3 crates/decodex-runtime/tests/native_qualification/response_identity.py
python3 crates/decodex-runtime/tests/native_qualification/project_guardian.py
```

These probes use loopback providers, synthetic account data and system-temporary homes. They close their own child processes and servers. They do not establish installed-app or live-provider acceptance. Keep raw output in the native run directory; keep current requirements, fixture implementations, hashes and limits in the repository.

A hook start event can precede native trust verification. The hook regression checks actual script effects after declaration or script changes, enablement changes and restart. The parallel tool regression requires both requests before either reply and verifies result order. The MCP status regression waits for discovery to finish, then checks opaque first-page metadata through tool pagination, status pagination and restart.

Use the current [qualification record](../../automations/upstream/qualification.json) for the source-test helper recipe and exact registered identities. Its current source cohort uses a `dev-small` app-server helper. Helper-spawn cases also require the exact signed candidate CLI through `CARGO_BIN_EXE_codex`; `CARGO_BIN_EXE_codex-app-server` identifies the source helper. Record the helper and production CLI separately and preserve the original assertions and timeouts. Older private-hook launchers apply only to their recorded source snapshot; do not copy their private configuration into the production CLI.

## Project, Work and global Guardian validation

The exact commands and selected identities are in `retained_project_agent_work` in the [retained index](../../automations/upstream/retained-evidence.json). Prepare the selected package with `DECODEX_CODEX_DEVELOPMENT_RUNTIME` and `cargo +stable make prepare-codex` before native qualification. Set both `DECODEX_TEST_CODEX_BINARY` and `DECODEX_NATIVE_BINARY` to its CLI for the native Nextest matrix. The Python Project/Guardian probe uses `QUALIFIED_CODEX_BINARY`, not either Nextest variable. Keep packaged helpers intact and use an existing temporary output directory.

```sh
cargo +stable make check-rust
cargo +stable make lint-rust
cargo +nightly fmt --all -- --check
cargo +stable make test-local-database
cargo +stable nextest run --locked -p decodex-database --all-features --no-fail-fast
cargo +stable nextest run --locked --workspace --all-targets --all-features --no-fail-fast
```

The original architecture qualification passed workspace checking, strict lint for 13 packages, formatting and the schema56/89-table local database gate. The database suite executed 188 tests successfully. The native selected matrix executed 13 successfully, excluding 985. The transport/receipt selection executed 28 successfully, excluding 970. Focused UI/runtime checks passed 6; Python checks passed 40; CLI diagnostics passed 2. Selected tests had zero unmatched identities. These cohorts overlap.

The original complete workspace run executed 2918 tests: 2916 passed, including one leaky pass, and two native audio ICE connection tests failed; 1154 were skipped. Preserve that failed receipt. The separate `native_media_followup` records a later current-main full run with 2918 passed, 1154 skipped and no leaky pass. Both media tests also passed 20 repetitions on current main and on the pre-change baseline, 40 executions per source with zero retries. Matching source and feature graphs plus historical firewall events support an external admission cause. Recovered lock and firewall history places the failures during locking; the known-allow package-only baseline passed in that interval. A new controlled lock or firewall-denial experiment was not run. Use the [native media timeout procedure](../integrations/subscription-voice.md#diagnose-native-media-ice-timeouts) before changing audio code or test deadlines.

### Native Project and nested Work follow-up

The `native_project_and_work_followup` entry records the next qualification against the same selected CLI and helpers. With both native binary variables set as described above, run its exact seven-test selection:

```sh
cargo +stable nextest run --locked -p decodex-runtime --all-features \
  --run-ignored only \
  -E 'test(native_project_reconciles_retained_conversation_and_survives_restart) or test(native_nested_code_mode_work_attribution_survives_store_reopen) or test(native_v1_nested_code_mode_keeps_exact_work_attribution) or test(native_child_completion_wakes_idle_root_once) or test(native_v1_work_attribution_uses_the_native_executor) or test(project_work::)' \
  --test-threads 1 --no-fail-fast
```

All seven executed and passed; 996 were excluded and no selector was unmatched. The tests cover retained native Project membership and real app-server restart, membership conflict isolation, direct/native V1 execution, permitted nested V1/V2 attribution, durable input recovery and V2 wake. V2 nesting uses explicit native configuration; the default direct-only policy remains unchanged. The recovery case reopens the store at a reconstructed durable boundary, not after a physical crash.

The follow-up also passed 189 database tests, 4 focused runtime guards, the schema57/90-table gate, workspace checking, strict lint for 13 packages, nightly formatting, Nextest TOML formatting, 40 Python tests and 2 CLI diagnostic tests. The final workspace/all-targets/all-features run passed 2921 tests, skipped 1157 and had no LEAK. Counts overlap. Commands, exact identities and selected source/component/schema digests remain in the portable index.

Its initial full run had three failures: two unchanged short-frame UI checks and an old fixture missing native `projectId`. The fixture now follows the real response. The same UI executable passed five isolated runs of each test. `.config/nextest.toml` extends existing runner reservation to those two selectors, with all assertions and default test selection retained. The initial passed resume test had a LEAK with unknown cause; the clean final run does not explain or replace it.

These checks do not install or start the product, spend real account capacity or measure live model judgment. Native Project data and membership do not establish official desktop saved-Project/sidebar presentation. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md) for native V1/V2, missing private action association, bounded evidence and installed-app limits.

## Prepare Codex components

Development uses the fixed reviewed commit in `codex-runtime.lock.json`. Prepare it explicitly once:

```sh
cargo make prepare-codex
```

The command reuses or repairs a verified component. If no usable artifact exists, it builds the exact source archive with stable Rust and locked upstream dependencies, then imports the signed package. To reuse an already qualified package instead of rebuilding it, set `DECODEX_CODEX_DEVELOPMENT_RUNTIME` to that package directory for the command. The archive, executable, helper commit and generated schema are checked. Native behavior qualification remains required when changing the reviewed pin.

For a formal release, set `DECODEX_CODEX_RELEASE` to an exact official version and run `cargo make pin-codex`. This writes `codex-runtime.release.lock.json` only after official package verification and confirmation that its commit contains the current development baseline. An older Release is rejected. Run the relevant native regressions against that package before approving the pin.

```sh
cargo +stable test --locked -p decodex-components
```

The ignored tests in `tests/official_release.rs` download real official packages into a temporary store. The ignored development test requires `DECODEX_TEST_DEVELOPMENT_RUNTIME` to identify the qualified package. These tests cover failed downloads, wrong commits and hashes, offline reuse, failed import preservation and corrupt cache repair without installing the app.

## Diagnostics and macOS packaging

```sh
cargo +stable run -p decodex-cli -- --version
cargo +stable run -p decodex-cli -- status
cargo +stable run -p decodex-cli -- --output json doctor
xcode-select --print-path
xcodebuild -version
scripts/macos/stage_decodex_app.sh
scripts/macos/test_decodex_app_stage.sh
```

A full Xcode installation and its Metal toolchain are required for GPUI. The scripts use the selected developer directory unless `DEVELOPER_DIR` is set. No Beta-specific path is required. The canonical stage script requires valid configured signing authority and rejects ad-hoc signing. It builds one app with the unified helper, native-client FFI, Swift menu-bar library and Sparkle. It embeds a runtime requirement, not the Codex binary or schemas. Neither staging nor application startup compiles Codex. Development staging uses `codex-runtime.lock.json`; set `DECODEX_BUILD_CHANNEL=release` for official Release staging, which requires `codex-runtime.release.lock.json` to cover the current baseline. Staging does not install the app. `DECODEX_SPARKLE_PUBLIC_KEY` supplies the public update key when configuring a distributable app; the private signing key is not an app input.

The staging test verifies payload counts, Info.plist, signatures, team identity, native ABI and a deliberately mismatched ABI fixture. Python reads piped entitlement bytes explicitly for compatibility. A passing package test is not live UI, speech, release notarization or installation acceptance.

An explicit `DECODEX_RUNTIME_LOCK_PATH` can select a prepared requirement for development staging. This selection does not qualify another artifact or compile Codex. Release staging still checks its selected official requirement against the development baseline.

## Native macOS tests

Run `scripts/macos/test_native_app.sh` for Swift tests that use the Rust native library. The script builds `decodex-app-client-ffi` with stable Rust and the lockfile, then gives the test fixture its exact debug-library path. It accepts normal `swift test` arguments, including `--filter`. The product continues to load only its bundled library; the test path is not a product override. Journal tests use temporary files and do not spend Reset Cards.

## Repository and auxiliary checks

```sh
cargo make check
cargo make test-automations
python3 automations/decodex/scripts/config/evaluate_automations.py --repo-only --json
cargo +stable test -p radar
cargo +stable test -p decodex-publisher
```

Makefile.toml owns the complete gate and tool-specific formatter/linter settings. Use repository lockfiles and already-managed tools. A missing prerequisite is not a source defect and does not authorize arbitrary toolchain replacement.

## Evidence boundaries

Report source checks, unit tests, signed build, visual acceptance, provider acceptance, PR merge and installed release separately. For UI changes test focus, typing, selection, scrolling, panel transitions and task switching. Preserve failure output; screenshots that fail or return blank do not establish that the app itself is blank.

### Nextest exit markers

A `LEAK` marker can accompany a passed test when its captured output remains open after process exit. Diagnose the exact runner, test cohort and pipe owners before attributing the marker to application cleanup. The [historical diagnosis](https://github.com/acg-box/decodex/blob/f6640838f0997493704ff87e46205b46a700eef2/automations/upstream/retained-evidence.json) records a macOS Nextest 0.9.143 capture-inheritance reproduction, its upstream fix and separate isolated and serial controls. The earlier component qualification separately records a passed in-memory test with a LEAK marker and a passing isolated rerun without the marker. The Project/Work complete suite separately records a leaky pass in `ui_working::tests::appearance_and_disappearance_share_one_continuous_fade`; its cause was not established. Neither result establishes the cause of a future marker. The original event has no pipe trace. Do not suppress the test, lower the timeout or claim that an unexecuted runner upgrade has passed.

[Wiki maintenance](wiki-maintenance.md) describes the separate documentation lifecycle.

## Documentation-only work

For documentation consolidation, validate page Claims, retained subject coverage, relative links and source paths. Do not rebuild the desktop or launch provider fixtures solely for a Markdown move. Keep historical test counts attached to their original revision. System-temporary fixtures must be cleaned after use; HOME is not a test-output directory.
