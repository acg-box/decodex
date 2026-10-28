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
  - id: openwiki-source-ca95ba5564216bb4898c8392
    resource: repo://apps/decodex-gpui/src/native_voice_online_tests.rs
  - id: openwiki-source-f7ecbc6c02d8c1b7873858d6
    resource: repo://automations/upstream/checkpoint.test.mts
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-8b67a68a7ef89772aa8180d1
    resource: repo://automations/upstream/README.md
  - id: openwiki-source-3127fd4c7b2aefcc9cea7961
    resource: repo://automations/upstream/retained-contracts.json
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
  - id: openwiki-source-8deed3df92ee012fa13bfea8
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_permission_tests.rs
  - id: openwiki-source-68238f7343c9fc1bd11783f3
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_recap_socket_tests.rs
  - id: openwiki-source-9976259de97569ef11093ba6
    resource: repo://crates/decodex-runtime/src/account_launch/process_native_component_tests.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-5af40fd50adccf5d93ff82a0
    resource: repo://crates/decodex-runtime/src/agent/tests/capacity.rs
  - id: openwiki-source-24139c353e393a9759b0267a
    resource: repo://crates/decodex-runtime/src/agent/tests/native_permissions.rs
  - id: openwiki-source-275dc903aab820a109a196b4
    resource: repo://crates/decodex-runtime/tests/fixtures/project_work_acceptance.json
  - id: openwiki-source-ba12412e93bca80301ac28a3
    resource: repo://crates/decodex-runtime/tests/native_qualification/apps_recovery.py
  - id: openwiki-source-30f880169ba4c3067a852d8f
    resource: repo://crates/decodex-runtime/tests/native_qualification/command_hooks.py
  - id: openwiki-source-b1a8e98eb39febad19f64bda
    resource: repo://crates/decodex-runtime/tests/native_qualification/dotenv_startup.py
  - id: openwiki-source-88b81a215b1bac061dad80ec
    resource: repo://crates/decodex-runtime/tests/native_qualification/model_errors.py
  - id: openwiki-source-e7a53f398340c29957bd81d3
    resource: repo://crates/decodex-runtime/tests/native_qualification/native_session.py
  - id: openwiki-source-8e6f29de9ed0d4825985ec62
    resource: repo://crates/decodex-runtime/tests/native_qualification/project_guardian.py
  - id: openwiki-source-2fee1dea7ba19d72acce7a3c
    resource: repo://crates/decodex-runtime/tests/native_qualification/proxy_headers.py
  - id: openwiki-source-aac8f76a32012d4a3e5a445d
    resource: repo://crates/decodex-runtime/tests/native_qualification/terminal_controls.py
  - id: openwiki-source-b30d382781a313611ecd221f
    resource: repo://crates/decodex-runtime/tests/native_qualification/thread_settings.py
  - id: openwiki-source-157a50123cd9da439c18985d
    resource: repo://crates/decodex-runtime/tests/native_qualification/usage.py
  - id: openwiki-source-042bf1b2b00d2d0ac4dced71
    resource: repo://crates/decodex-webrtc-build/src/archive.rs
  - id: openwiki-source-87ec62f51f8e5d9a99b4bb6e
    resource: repo://crates/decodex-webrtc-build/tests/qualify_trust_roots.py
  - id: openwiki-source-b7b32ffdb5400494a6a36b80
    resource: repo://crates/decodex-webrtc-build/tests/trust_roots.json
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
  - id: openwiki-source-90e2e66c4c17531650b4f54e
    resource: repo://scripts/macos/prepare_voice_acceptance.py
  - id: openwiki-source-2fc96071d03c8f12c8c6fb3a
    resource: repo://scripts/macos/run_native_voice_tests.py
  - id: openwiki-source-3b57179b92b257bc3fff51a1
    resource: repo://scripts/macos/stage_decodex_app.sh
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
  - id: openwiki-source-e0e48fb115095577a43dbc91
    resource: repo://scripts/macos/test_native_app.sh
  - id: openwiki-source-e201f60894c32a914dac99ee
    resource: repo://scripts/vnext/run_native_regression.py
  - id: openwiki-source-d45a647d565211cf9b5cb067
    resource: repo://scripts/vstyle_audit.py
generated: { by: "codex", at: "2026-10-28T08:28:06.287Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-28T08:48:31.048Z
---


# Commands and validation

Use the smallest check that proves the changed contract. Broaden testing for shared runtime, protocol, database or packaging changes. The repository uses stable Rust for builds and tests; formatting uses the unversioned `nightly` channel selected by `Makefile.toml`. Do not select numbered compiler versions or dated nightly versions. Run `cargo make fmt-rust` to format and `cargo make fmt-rust-check` to check formatting.

## Source and runtime owners

| Owner | Responsibility |
| --- | --- |
| `database/` | Current SQLite schema, transactions and restart fixtures |
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

## Signed native WebRTC checks

On macOS, set `DECODEX_APP_SIGN_IDENTITY` to the name or fingerprint of an existing development certificate in the local Keychain:

```sh
DECODEX_APP_SIGN_IDENTITY="$EXISTING_DEVELOPMENT_IDENTITY" cargo +stable make test-native-voice
```

This explicit task builds the locked GPUI test target with all features, signs a separate test copy at a stable target path and uses Nextest build reuse to run four exact transport tests. A missing or ignored test fails selection. An unavailable signature stops execution. Cargo's original artifact, general test commands, native WebRTC packet flow, assertions and zero-retry policy stay intact. The signed copy is serialized so another invocation cannot replace it during a test run.

Allow the first system firewall prompt for `decodex-native-voice-tests`, then rerun the command. The measured certificate and path retained their allow decision across different compiled code versions. Other certificates, paths, systems and firewall policies remain outside that qualification. The tests use synthetic PCM with hardware I/O disabled; separate physical-device and online-provider prerequisites still apply. See [the signing evidence and limits](../integrations/subscription-voice.md#run-native-webrtc-tests-with-a-development-signature).

For full macOS workspace coverage, run the other tests in the ordinary lane and
run these four tests through the signed task. Derive their exact filter from the
checked-in runner:

```sh
DECODEX_VOICE_FILTER="$(python3 -c 'import runpy; print(runpy.run_path("scripts/macos/run_native_voice_tests.py")["FILTER"])')"
cargo +stable nextest run --locked --workspace --tests --examples --benches --all-features -E "not ($DECODEX_VOICE_FILTER)"
DECODEX_APP_SIGN_IDENTITY="$EXISTING_DEVELOPMENT_IDENTITY" cargo +stable make test-native-voice
```

Report both lanes and their selected counts. The signed task checks that every
selected transport test exists and is not ignored.

## Qualify native dependency artifacts

Freeze dependency versions and immutable Git refs before qualification. Use the
repository lockfiles and existing managed tools. Resolve new graphs without
executing install scripts, inspect changed source and build entrypoints, then run
the affected repository checks. Keep direct declarations in inherited minor-line
tables and let the package managers regenerate their locks.

For the official WebRTC archives, download the selected arm64 and x64 release
assets into system-temporary paths. Set `DECODEX_WEBRTC_ARM64_ARCHIVE` and
`DECODEX_WEBRTC_X64_ARCHIVE` to those files. Run the retained digest and strict-header
test:

```sh
cargo +stable test --locked -p decodex-webrtc-build archive::tests::official_macos_archives_pass_digest_and_strict_header_validation -- --exact --ignored --nocapture
python3 crates/decodex-webrtc-build/tests/qualify_trust_roots.py --archive "$DECODEX_WEBRTC_ARM64_ARCHIVE" --sha256 153a1f1a268b812b4a501deda6505037f29481691efec52be817f560ff09e206
python3 crates/decodex-webrtc-build/tests/qualify_trust_roots.py --archive "$DECODEX_WEBRTC_X64_ARCHIVE" --sha256 c53d7d20277f7729956854f1268c372360f08732520136b559034eedae0980a2
```

Use the hashes for the selected release in `pinned_digest` and the
`workspace_dependencies` section of the [qualification record](../../automations/upstream/qualification.json)
when that selection changes. The probe checks certificate DER bytes in the actual
static library and retains a positive control. Its optional baseline arguments
verify a previous library against its recorded digest. Header and library checks
do not execute TURN/TLS or qualify physical audio and providers. Run the signed
real WebRTC lane for packet flow against the rebuilt artifact.

Keep workspace dependency results separate from the selected Codex CLI and its
helpers. Record residual upstream constraints and the actual full-style audit
outcome. A baseline comparison does not turn a failed style audit into a pass.

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

Use [the stable contract catalog](../../automations/upstream/retained-contracts.json) for retained requirements, exact test names and original limits. The [current evidence router](../../automations/upstream/retained-evidence.json) links the selected runtime qualification. Choose only probes affected by the changed consumer; the command list below is not a daily test cohort. Use its command and environment with the candidate CLI and its packaged helpers. Register the selected tests first and include ignored native fixtures explicitly. Require every listed test to execute. Keep source tests, local projection tests and actual CLI tests in separate receipts.

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
python3 crates/decodex-runtime/tests/native_qualification/command_hooks.py
python3 crates/decodex-runtime/tests/native_qualification/dotenv_startup.py
```

These probes use loopback providers, synthetic account data and system-temporary homes. They close their own child processes and servers. They do not establish installed-app or live-provider acceptance. Keep raw output in the native run directory; keep current requirements, fixture implementations, hashes and limits in the repository.

The command hook probe checks the command directory in the `PreToolUse`, `PermissionRequest` and `PostToolUse` payloads. The hook process keeps the project directory, while each payload identifies the nested command directory. It checks one local command without a model retry; it does not qualify a remote executor.

The startup probe checks both a missing `.env` and a directory at that path. The settings probe separately checks saved tier/summary and permission recovery despite stale turn context in legacy and paginated history. The model-error probe keeps `policyCapacityUnavailable` distinct from overload; the HTTP fixture permits three native requests with `request_max_retries=2`, while the SSE failed-response case permits one. Neither adds a Decodex automatic retry.

A hook start event can precede native trust verification. The hook regression checks actual script effects after declaration or script changes, enablement changes and restart. The parallel tool regression requires both requests before either reply and verifies result order. The MCP status regression waits for discovery to finish, then checks opaque first-page metadata through tool pagination, status pagination and restart.

Read the current [qualification record](../../automations/upstream/qualification.json) for the recorded source-test helper recipe and exact registered identities. Run an upstream selection only for a concrete compatibility diagnosis or missing retained-fix proof; do not inherit that cohort as a daily gate. The recorded source cohort uses `dev-small` helpers. Nextest can replace requested `CARGO_BIN_EXE_*` values with its own build outputs; those tests do not prove that the signed production CLI ran. Use the Decodex native cohort and standalone probes for exact CLI checks. For the signed Code Mode helper, invoke the compiled `grpc_tcp` test binary directly with both documented helper path variables. Record each actual artifact separately and preserve the original assertions and timeouts. Older private-hook launchers apply only to their recorded source snapshot; do not copy their private configuration into the production CLI.

## Memory, collaboration history and actual usage

`selected_read_views` in the [stable contract catalog](../../automations/upstream/retained-contracts.json) records the exact regression identities, source entrypoints, selected CLI and helper digests, generated schema digest and scope limits for these three views. Run preparation explicitly. Set `DECODEX_TEST_CODEX_BINARY` to the prepared CLI; keep its packaged helpers available.

```sh
DECODEX_CODEX_DEVELOPMENT_RUNTIME=<qualified-runtime-directory> cargo make prepare-codex
DECODEX_TEST_CODEX_BINARY=<prepared-cli> cargo +stable nextest run --locked   -p decodex-runtime --lib --all-features --run-ignored only   -E 'test(installed_native_memory_view_selects_versions_without_model_calls_or_fallback) | test(installed_native_usage_restores_agent_baseline_after_cold_resume)'   --test-threads 1 --no-fail-fast
cargo +stable nextest run --locked -p decodex-gpui --bin decodex-gpui --all-features   -E 'test(memory_view_copies) | test(answered_question) | test(independent_repair_cycles) | test(usage_labels)'   --test-threads 1 --no-fail-fast
cargo +stable nextest run --locked --workspace --all-targets --all-features   --test-threads 4 --no-fail-fast
python3 -m unittest tests/scripts/test_vnext_architecture.py tests/scripts/test_account_login_architecture.py tests/scripts/test_gate_contract.py tests/scripts/test_vstyle_audit.py
python3 scripts/vnext/local_database_gate.py
cargo make fmt-rust-check
```

The qualification passed 2,998 workspace tests with 1,222 skipped, two exact-artifact native tests, four selected UI tests and 28 Python tests. The workspace run preceded one added memory copy/display test; the later four-test UI selection includes that test. Both selected native and UI cohorts had zero unmatched selectors. Strict Clippy passed for the five changed packages with the repository warning policy. Formatting passed. The database gate passed schema59 with 90 tables after its inventory was aligned with the existing Rust migration ledger; this change adds no migration.

The native memory fixture checks version selection, disabled/missing/blank states and an unstarted thread with zero model requests. The usage fixture checks retained totals across a native process restart. These tests use temporary homes and a local Responses server. Upstream memory test bodies were read; upstream Rust tests were not executed in this qualification. The full `python3 scripts/vstyle_audit.py` run failed with 16,517 findings at implementation commit `1beae0c0415c28ef45d8afaa1bd8117832c62c25`. No exemption or all-inherited classification is recorded. Keep that result separate from the passed checks and the independent repository-wide style cleanup.

These results establish source and fixture behavior. They do not establish installed-app interaction, real-provider billing, complete historical Work cost or elapsed time. A rebuilt runtime requires new artifact/schema observations and affected native regressions. See [Selected read-only views](../testing/upstream-acceptance-boundaries.md#selected-read-only-views).

## Project, Work and global Guardian validation

The exact commands and selected identities are in `retained_project_agent_work` in the [stable contract catalog](../../automations/upstream/retained-contracts.json). Prepare the selected package with `DECODEX_CODEX_DEVELOPMENT_RUNTIME` and `cargo +stable make prepare-codex` before native qualification. Set both `DECODEX_TEST_CODEX_BINARY` and `DECODEX_NATIVE_BINARY` to its CLI for the native Nextest matrix. The Python Project/Guardian probe uses `QUALIFIED_CODEX_BINARY`, not either Nextest variable. Keep packaged helpers intact and use an existing temporary output directory.

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

### Source-built Project and Guardian acceptance

`retained_project_agent_work.live_acceptance` records the later lifecycle repairs and their exact selectors. The final complete workspace run passed 2924 tests with 1159 skipped and no LEAK. Nine exact-artifact native tests, strict lint for 13 packages, formatting and the schema59 integrity checks passed. The two earlier leaky passes were unchanged tests; both passed in isolation. Their original marker cause remains unknown.

Use `crates/decodex-runtime/tests/fixtures/project_work_acceptance.json` for the reusable live scenario. It supplies material data, discussion and assignment prompts, the requirement change and expected arithmetic. Use an isolated app data root, a model returned by the selected runtime's catalog, and caller-owned permitted material directories. Keep authentication and detailed receipts outside Git. The source-built scenario used two Projects, two Agents and four Works, including same-request repair, restart and global Guardian.

Build the source app with the repository staging command. This creates a signed test bundle; it does not install or publish the app:

```sh
DECODEX_APP_STAGE_DIR="$(mktemp -d)/stage" sh scripts/macos/stage_decodex_app.sh
```

Record the bundle's actual service build identity and component digests separately from the selected native CLI and helpers. Check the native Project catalog and membership separately from official desktop presentation. The latter remains unobserved. Record encrypted task-name attribution as routing proof, not plaintext verification. The control follow-up verified actual Pause/Resume buttons, hold persistence, native rejection while held, and completion after release. It also verified Full Access with a denied/allowed/denied write sequence and a restart between the allowed writes.

Use `controls_followup` in the reusable scenario for these probes. Set both `DECODEX_TEST_CODEX_BINARY` and `DECODEX_NATIVE_BINARY` to the same verified prepared CLI. The three selected native tests use two environment-variable names:

```sh
export DECODEX_NATIVE_BINARY="$DECODEX_TEST_CODEX_BINARY"
cargo +stable nextest run --locked -p decodex-runtime --run-ignored only --test-threads 1 \
  -E 'test(installed_permission_selection_publishes_and_survives_native_restart) or test(installed_named_permission_selection_during_active_turn_survives_restart) or test(native_agent_resume_preserves_selected_profile_policy_and_cwd)'
cargo +stable nextest run --locked -p decodex-database \
  -E 'test(works::tests::user_holds_and_scoped_materials_do_not_grant_private_history)'
```

The control follow-up passed all three native tests and the selected hold test. Its earlier missing-variable and global-reviewer fixture failures remain in the separate receipt. These results do not replace the earlier full workspace suite. Cold native profile controls can be unavailable until the context loads; do not resume work implicitly to make a read succeed. For old encrypted helpers, compare the parent call and child input, then read the child's own output. Matching encrypted delivery establishes transport, not the prepared plaintext or a strong Work binding.

The later `installed_acceptance` entry records authorized local development installation separately. Its two independent-Agent UI regressions failed before repair. The final UI package passed 1025 tests with 1031 skipped; complete runs retained cleanup markers in three unchanged tests, and all three isolated controls passed without a marker. Twelve selected native regressions had zero unmatched selectors. Strict lint for 13 packages and formatting passed. Use the entry's exact artifact identities and commands; do not count the older 2924-test workspace receipt as a fresh run of these UI repairs.

### Synthetic online voice acceptance

These ignored tests require explicit authority to send synthetic audio through an existing subscription connection. Use a signed service and the verified selected CLI and helpers. Prepare the runtime explicitly in a fresh system-temporary native home. Isolate both the product root and native home; setting only the product root leaves native history shared.

Use the existing local bootstrap owners for the private configuration, server identity and task-scoped [execution authorization](../../crates/decodex-core/src/process_generation.rs). Enroll the existing login through the account CLI without copying the user's business database. Remove temporary authentication copies after enrollment. Start exactly one read-only Agent named `voice-acceptance`, with the instruction to reply only `VOICE_READY` and use no tools. Wait until it is idle. Keep the service's native HOME separate from the host tools' home and configuration.

Set `DECODEX_VOICE_ACCEPTANCE_ROOT` to that isolated product root. Set `VOICE_FIXTURE_INPUT` to a system-temporary input directory. The prepared-root marker is a deliberate fixture guard, not a product permission override:

```sh
printf '%s\n' 'Decodex synthetic voice acceptance' > "$DECODEX_VOICE_ACCEPTANCE_ROOT/synthetic-voice-fixture"
python3 scripts/macos/prepare_voice_acceptance.py "$VOICE_FIXTURE_INPUT"
export DECODEX_VOICE_ACCEPTANCE_INPUT="$VOICE_FIXTURE_INPUT"
cargo +stable nextest list --locked -p decodex-gpui --bin decodex-gpui --all-features \
  --run-ignored only -E 'test(online_synthetic_)'
cargo +stable nextest run --locked -p decodex-gpui --bin decodex-gpui --all-features \
  --run-ignored only -E 'test(online_synthetic_)' --test-threads 1 --retries 0 --no-fail-fast
```

The generator uses an already installed macOS Samantha English voice. It creates mono PCM16 at 24 kHz and 48 kHz and records input digests. It does not install a voice or open a microphone. The test fixture refuses a missing marker or a service with more than its single fixture Agent. The dictation test checks final words and unchanged Agent history. The live test uses manual PCM, checks final assistant text and decoded audio, then requires Ended after Stop.

The final recorded cohort passed two tests, skipped 1023 and had no unmatched selector, retry or LEAK. Strict lint for 13 packages and formatting passed. Keep real-provider results separate from local hardware capture and installed UI checks. The user's separate playback observation was audible but quiet; it did not change system volume or routing. Preserve compact digests and domain receipts, close the owned service, then remove temporary homes, credentials, components and audio. See `retained_project_agent_work.online_voice_acceptance` and the [voice boundary](../integrations/subscription-voice.md#authorized-synthetic-online-checks).

## Attached-branch native regression fixtures

Use `scripts/vnext/run_native_regression.py` with an explicit qualified runtime directory and one exact fixture selector per invocation. It creates a short canonical system-temporary HOME, preserves host Cargo and Rustup locations, runs the explicit component preparation command, and removes the fixture afterward. Preparation uses a separate native home so that it cannot populate the account fixture. The public-socket fixture uses the owned checkout as its working directory; product directory checks remain active.

```sh
DECODEX_TEST_NATIVE_FORK=1 python3 scripts/vnext/run_native_regression.py \
  --runtime "$QUALIFIED_RUNTIME" \
  --filter 'test(installed_recap_public_socket_preserves_parent_and_exact_request_identity)'
```

The `retained_attached_forks` record in the [stable contract catalog](../../automations/upstream/retained-contracts.json) lists the other four exact-artifact native tests and all reproducible commands. The final cohort passed five native tests, 57 focused backend tests and six focused UI tests with zero unmatched selectors. The complete workspace run passed 3006 tests with 1227 skipped and one passed-test LEAK; the affected composer test then passed in the clean focused cohort. These overlapping selections do not establish the cleanup cause.

Strict Clippy for five affected packages, nightly formatting, 28 Python checks, the schema59 database gate and the full 1076-file style audit passed. This style result is separate from the earlier read-only-view audit failure. The native provider is synthetic. These checks did not install or launch the desktop app, qualify real-provider behavior, migrate the runtime baseline, advance upstream review or change the schedule.

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

## Qualify components and original EGM

Use the verified prepared Codex executable and a private evidence directory:

```sh
DECODEX_TEST_CODEX_BINARY="$PREPARED_CODEX" DECODEX_TEST_EGM_EVIDENCE="$EGM_EVIDENCE" cargo +stable nextest run --locked -p decodex-runtime --lib --run-ignored only -E 'test(isolated_original_egm_completes_an_approved_review_repair_cycle)' --test-threads 1
```

The fixture installs the original plugin in a temporary native home. It retains an unchanged target at the first decision gate, sends a separate repair approval, then explicitly starts a second fresh review and halts the loop. It saves both target versions, reports and native events. Source-read and completion checks do not rely on the parent's summary. The fixture uses process-scoped existing subscription authentication and consumes model quota; it needs no API key. Its bounded workflow does not prove every EGM scenario or installed-app appearance.

The native plugin adapter and component worker separately test installation, updates, failure preservation, removal and orphan cleanup. They retain unrelated plugins and user artifacts.

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

The automation test task uses the managed Node version from `site/.nvmrc` and runs checkpoint behavior regressions before the existing portfolio tests. It checks contiguous publication, stale input, exact replay, explicit archival and read-only inspection. Use the [checkpoint publisher](codex-upstream-autopilot.md#publish-a-checkpoint) for native state changes. Automation-control changes need these affected checks, not a Codex build or app/provider suite.

## Evidence boundaries

Report source checks, unit tests, signed build, visual acceptance, provider acceptance, PR merge and installed release separately. For UI changes test focus, typing, selection, scrolling, panel transitions and task switching. Preserve failure output; screenshots that fail or return blank do not establish that the app itself is blank.

### Nextest exit markers

A `LEAK` marker can accompany a passed test when its captured output remains open after process exit. Diagnose the exact runner, test cohort and pipe owners before attributing the marker to application cleanup. The [historical diagnosis](https://github.com/acg-box/decodex/blob/f6640838f0997493704ff87e46205b46a700eef2/automations/upstream/retained-evidence.json) records a macOS Nextest 0.9.143 capture-inheritance reproduction, its upstream fix and separate isolated and serial controls. The Project/Work complete suite separately records a leaky pass in `ui_working::tests::appearance_and_disappearance_share_one_continuous_fade`; its cause was not established. Neither result establishes the cause of a future marker. The original event has no pipe trace. Do not suppress the test, lower the timeout or claim that an unexecuted runner upgrade has passed.

[Wiki maintenance](wiki-maintenance.md) describes the separate documentation lifecycle.

## Documentation-only work

For documentation consolidation, validate page Claims, retained subject coverage, relative links and source paths. Do not rebuild the desktop or launch provider fixtures solely for a Markdown move. Keep historical test counts attached to their original revision. System-temporary fixtures must be cleaned after use; HOME is not a test-output directory.
