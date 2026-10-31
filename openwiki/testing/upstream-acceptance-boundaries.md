---
type: Reference
title: "Upstream integration acceptance boundaries"
description: "Source, native fixture, signed desktop, installation and physical voice evidence boundaries."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-95772fa38252ded838ec3a2f
    resource: repo://.config/nextest.toml
  - id: openwiki-source-0a61b25348131fe1bb6e1d12
    resource: repo://apps/decodex-gpui/src/agent_context_settings_wire_tests.rs
  - id: openwiki-source-d56ea7499f2ed784b1b6c970
    resource: repo://apps/decodex-gpui/src/agent_conversation_tab_tests.rs
  - id: openwiki-source-85ab533d66ee904d83d8a21a
    resource: repo://apps/decodex-gpui/src/agent_item_context_tests.rs
  - id: openwiki-source-77ad5024b567271a0f618fd7
    resource: repo://apps/decodex-gpui/src/agent_list_branch.rs
  - id: openwiki-source-89a6e0bcef9377afc01425eb
    resource: repo://apps/decodex-gpui/src/agent_list_status_tests.rs
  - id: openwiki-source-1b7747f8ae67c32e5670817d
    resource: repo://apps/decodex-gpui/src/agent_portfolio_tests.rs
  - id: openwiki-source-0eea3c2bab20e4a9d8e3d80a
    resource: repo://apps/decodex-gpui/src/agent_prompt_confirm_wire_tests.rs
  - id: openwiki-source-218f7834832fc84c2fbc7cf7
    resource: repo://apps/decodex-gpui/src/agent_reconnect_queue_tests.rs
  - id: openwiki-source-032278b78f4f6cac0e654ef0
    resource: repo://apps/decodex-gpui/src/native_audio_owner_tests.rs
  - id: openwiki-source-ca95ba5564216bb4898c8392
    resource: repo://apps/decodex-gpui/src/native_voice_online_tests.rs
  - id: openwiki-source-d1f06f7b22d0c2eec328d95f
    resource: repo://apps/decodex-gpui/src/native_voice_transport.rs
  - id: openwiki-source-546812ff3637690f4a184b89
    resource: repo://apps/decodex-gpui/src/ui_preferences.rs
  - id: openwiki-source-3122dd472ce3039751b99877
    resource: repo://automations/decodex/scripts/config/tests/test_portfolio.py
  - id: openwiki-source-14193a66abfb7d3230f476bf
    resource: repo://automations/portfolio.toml
  - id: openwiki-source-f7ecbc6c02d8c1b7873858d6
    resource: repo://automations/upstream/checkpoint.test.mts
  - id: openwiki-source-a5b9d61b2b9a5f7492d602ac
    resource: repo://automations/upstream/current-review.json
  - id: openwiki-source-7dcbb082d2502f1ec4386c39
    resource: repo://automations/upstream/prompts/maintainer.md
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-8b67a68a7ef89772aa8180d1
    resource: repo://automations/upstream/README.md
  - id: openwiki-source-3127fd4c7b2aefcc9cea7961
    resource: repo://automations/upstream/retained-contracts.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-16e6e7418602542c69565bde
    resource: repo://Cargo.lock
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-091210eb4586f0ba4c9d751d
    resource: repo://crates/decodex-account-login/src/system_proxy.rs
  - id: openwiki-source-02bd3cfc9d4db9d1d1d8fd59
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences_tests.rs
  - id: openwiki-source-0d4c68456f2e50c6fc7ad5eb
    resource: repo://crates/decodex-components/src/install.rs
  - id: openwiki-source-8bbd4ba0cf275fa1b4da9acf
    resource: repo://crates/decodex-components/tests/development.rs
  - id: openwiki-source-08dbb31668b58707418687d2
    resource: repo://crates/decodex-components/tests/official_release.rs
  - id: openwiki-source-aee901c2feaee74f17637372
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_compaction_tests.rs
  - id: openwiki-source-8fd14b27958d10f11e115032
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_desktop_acceptance.rs
  - id: openwiki-source-4a57fba4bea69171318c6323
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_fork_tests.rs
  - id: openwiki-source-8d32d49a0bc31600300d9335
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_lookup_tests.rs
  - id: openwiki-source-19796bbb5939eefbc15bb222
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_mcp_socket_tests.rs
  - id: openwiki-source-e5be8be8499e6ccee0cf7ad0
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_memory_tests.rs
  - id: openwiki-source-32afc8038b2c3b7df030c573
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_prediction_tests.rs
  - id: openwiki-source-1b314e86ab2f1adbc0ede4c8
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_project_sync_tests.rs
  - id: openwiki-source-725813d500ec103482924456
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_project_work_tests.rs
  - id: openwiki-source-21c0991a686dae637a3616cd
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_read_state_tests.rs
  - id: openwiki-source-68238f7343c9fc1bd11783f3
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_recap_socket_tests.rs
  - id: openwiki-source-ea794ca791dfc854f2ec0a89
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_usage_tests.rs
  - id: openwiki-source-f5d073da07bcb17ee416f3b5
    resource: repo://crates/decodex-runtime/src/account_launch/process_native_control_tests.rs
  - id: openwiki-source-b2869e6da778a74cb6afc667
    resource: repo://crates/decodex-runtime/src/account_launch/process.rs
  - id: openwiki-source-4e8482c0f2dbe370858a1b11
    resource: repo://crates/decodex-runtime/src/agent_context_settings_native_tests.rs
  - id: openwiki-source-fe4da151d2112b589ec8679c
    resource: repo://crates/decodex-runtime/src/agent_context_settings_tests.rs
  - id: openwiki-source-5bc08ad67fcdb104dfcde7bf
    resource: repo://crates/decodex-runtime/src/agent_permissions.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-a4a2658e06fa2cf48ff4589e
    resource: repo://crates/decodex-runtime/src/agent/global_guardian.rs
  - id: openwiki-source-283a40a004fda23504fad7e1
    resource: repo://crates/decodex-runtime/src/agent/result_messages.rs
  - id: openwiki-source-5af40fd50adccf5d93ff82a0
    resource: repo://crates/decodex-runtime/src/agent/tests/capacity.rs
  - id: openwiki-source-30c7edf894a639091ecbafc6
    resource: repo://crates/decodex-runtime/src/agent/tests/external_context.rs
  - id: openwiki-source-a7b97dd347fbe2f630692d12
    resource: repo://crates/decodex-runtime/src/agent/tests/native_agent_wake.rs
  - id: openwiki-source-24139c353e393a9759b0267a
    resource: repo://crates/decodex-runtime/src/agent/tests/native_permissions.rs
  - id: openwiki-source-1c25f1eb2ea8f33c9b46e78e
    resource: repo://crates/decodex-runtime/src/agent/tests/work_contract.rs
  - id: openwiki-source-64d7588c1af50398568a792e
    resource: repo://crates/decodex-runtime/src/agent/timeline.rs
  - id: openwiki-source-74145d84eb83d80ec3062595
    resource: repo://crates/decodex-runtime/src/agent/timeline/metrics.rs
  - id: openwiki-source-4f59431f3f6264a65208bdcb
    resource: repo://crates/decodex-runtime/src/agent/work_delivery.rs
  - id: openwiki-source-b1dd4c8ab85f342314daa6f1
    resource: repo://crates/decodex-runtime/src/agent/work.rs
  - id: openwiki-source-ec5c9f32d2135154f4297a49
    resource: repo://crates/decodex-runtime/src/conversation.rs
  - id: openwiki-source-6085af9603cb7ac0ff3ef1cf
    resource: repo://crates/decodex-runtime/src/work_portfolio.rs
  - id: openwiki-source-275dc903aab820a109a196b4
    resource: repo://crates/decodex-runtime/tests/fixtures/project_work_acceptance.json
  - id: openwiki-source-b1a8e98eb39febad19f64bda
    resource: repo://crates/decodex-runtime/tests/native_qualification/dotenv_startup.py
  - id: openwiki-source-07c9cc033899826079f8efb5
    resource: repo://crates/decodex-runtime/tests/native_qualification/large_catalog.py
  - id: openwiki-source-c772fc057f500f765d09a3c7
    resource: repo://crates/decodex-runtime/tests/native_qualification/log_storage.py
  - id: openwiki-source-88b81a215b1bac061dad80ec
    resource: repo://crates/decodex-runtime/tests/native_qualification/model_errors.py
  - id: openwiki-source-8e6f29de9ed0d4825985ec62
    resource: repo://crates/decodex-runtime/tests/native_qualification/project_guardian.py
  - id: openwiki-source-c6bea677adcde563202b65b9
    resource: repo://crates/decodex-runtime/tests/native_qualification/response_identity.py
  - id: openwiki-source-b30d382781a313611ecd221f
    resource: repo://crates/decodex-runtime/tests/native_qualification/thread_settings.py
  - id: openwiki-source-042bf1b2b00d2d0ac4dced71
    resource: repo://crates/decodex-webrtc-build/src/archive.rs
  - id: openwiki-source-9d02660394edc50f69d35816
    resource: repo://crates/decodex-webrtc-build/src/lib.rs
  - id: openwiki-source-87ec62f51f8e5d9a99b4bb6e
    resource: repo://crates/decodex-webrtc-build/tests/qualify_trust_roots.py
  - id: openwiki-source-b7b32ffdb5400494a6a36b80
    resource: repo://crates/decodex-webrtc-build/tests/trust_roots.json
  - id: openwiki-source-a6440a3d3a3c3121fd4c553f
    resource: repo://database/src/agent_output.rs
  - id: openwiki-source-2b705b409434252a7417e708
    resource: repo://database/src/usage_evidence_tests.rs
  - id: openwiki-source-d09dfdea9b65815f3a76b46c
    resource: repo://database/src/works/tests.rs
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
  - id: openwiki-source-e8c30c6f5849a5077d7b4aa5
    resource: repo://scripts/macos/build_codex_source.test.mts
  - id: openwiki-source-90e2e66c4c17531650b4f54e
    resource: repo://scripts/macos/prepare_voice_acceptance.py
  - id: openwiki-source-2fc96071d03c8f12c8c6fb3a
    resource: repo://scripts/macos/run_native_voice_tests.py
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
  - id: openwiki-source-e201f60894c32a914dac99ee
    resource: repo://scripts/vnext/run_native_regression.py
generated: { by: "codex", at: "2026-10-31T13:17:02.524Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-31T15:20:06.142Z
---

# Upstream integration acceptance boundaries

## Separate evidence levels

| Evidence | What it establishes | What it does not establish |
| --- | --- | --- |
| Source/consumer reconciliation | A capability has a current owner or an explicit retirement | Successful execution |
| Focused unit or socket fixture | The checked local contract and failure case | An external provider or real desktop interaction |
| Component integrity and repair | Exact archive, signed executable, helper revision, generated schema and cache recovery | All native behavior or a working public app-update feed |
| Installed-native isolated fixture | The named binary's behavior with the supplied synthetic environment | Every account, feature flag or platform |
| Signed desktop acceptance | The recorded artifact and exact interactions | Installation, notarization, public release or all UI surfaces |
| PR merge and remote readback | Code is in the specified main history | The user's installed app contains it |

## Current check owners

`Makefile.toml` defines workspace checks. Isolated native fixtures exercise a named Codex binary and must keep their declared environment and effect prerequisites. The signed bundle contract checks packaging, signatures and ABI compatibility; it does not exercise live microphone, network or focus behavior.

For native compatibility, record the upstream commit separately from the installed binary and generated schema. Test the actual consumer contract. A schema field or retained database row is not proof of a delivered UI or supported execution path.

Workflow-only maintenance uses the checkpoint and portfolio checks in `cargo +stable make test-automations`. This verifies maintenance bookkeeping and repository configuration; it does not require app, provider or upstream suites and does not qualify a different runtime. The portable index retains the executed identities and limits. Native definition readback is separate from repository validation, and neither resumes a paused schedule.

## Current fixed development qualification

Read the development lock, [current source review](../../automations/upstream/current-review.json) and [current qualification](../../automations/upstream/qualification.json) for the fixed target, exact CLI/helpers/schema, executed identities and limits. Keep source review, source-test execution, exact production-artifact tests, delivery and installed acceptance separate. Current run counts and hashes belong in those records, not repeated across Wiki pages.

The provider-refusal follow-up resolves [the former compatibility blocker](https://github.com/acg-box/decodex/issues/2575) through the supported native fork. The original cohort remains historical evidence; it is not a freshly rerun matrix. Source review is complete through the fixed target, but the runtime lock remains on the qualified baseline. HumanCommand compatibility and its affected qualification remain pending. No upstream test suite was executed for this follow-up.

The retired Codex startup-configuration diagnostic is absent from the current router. Its earlier execution remains historical proof. Valid named-permission defaults and an owned Recap working directory repair fixture admission without weakening existing behavior assertions. The exact-artifact development cache test also corrupts and repairs any advertised standalone executor.

The default execution scope is affected Decodex consumers against the prepared artifact and the applicable repository gate. Read relevant upstream implementation and test bodies for source review; execute upstream tests only for a concrete compatibility question or missing retained-fix proof. An unconsumed fixture failure remains a diagnostic limit and is not counted as passed. It does not create a Decodex repair item. This does not relax the evidence needed to advance the source-review cursor or qualify an artifact.

## Provider recovery qualification

The `provider_recovery` entry under `candidate_qualification` in the [qualification record](../../automations/upstream/qualification.json) binds the focused local tests, exact candidate and baseline native tests, component signatures and regenerated schemas to the implementation commit. Ordinary forks retain the source provider; cold resume follows native admission after that provider is restored. The new regression uses two loopback providers and verifies an explicit current-provider fork, preserved context after restart, unchanged source history and no inference until a separate send.

The UI and receipt tests cover source identity, configuration changes, cancellation, the durable destination choice and preservation of unsent input. The real public-socket fixture also prepares the mismatch before desktop launch. Its setup check substitutes `/usr/bin/true` for the desktop and is not UI acceptance. The [interactive rehearsal command](../operations/commands-and-validation.md#rehearse-provider-recovery) uses a real desktop window and an isolated temporary profile; its two provider names share one local backend. Neither fixture qualifies live accounts, provider policy migration, installed-app behavior or performance. No runtime pin, installation, release or schedule activation follows from these results.

## Observed turn timings

The delivered response details show locally observed activity intervals and explicit unknown values. The [portable record](../../automations/upstream/retained-contracts.json) identifies the source, exact selected native artifact and test entrypoints. Its headless cohort passed 1891 tests, and two selected presentation tests passed. These results do not establish installed-app interaction or live-provider timing. Historical activity durations are available only when the native history or an exact saved observation contains them.

The selected CLI's generated schema exposes a nullable total turn duration but no full critical-path categories. The source research found experimental category totals in native analytics, with disabled, incomplete and complete states. Its tests used modified research source and synthetic services. They do not establish a supported client subscription, a qualified newer artifact or complete inference timing. Continue to show unavailable values as Unknown.

## Native MCP sharing qualification

Five historical socket cases ran against the then-selected unmodified CLI from upstream source `abf4bb91ec49fbf443ccec0069f9cab5dd1462d8`. Three passed: compatible modern sharing, concurrent input routing, and an explicitly marked legacy helper with the modern protocol feature disabled. Two failed at thread startup: an unmarked helper returned an empty discovery result, and a marked legacy helper was used with the modern protocol feature enabled. An empty successful discovery result is a synthetic compatibility case, not a conforming method-not-found error or proof of a legacy server.

Two proxy clients sharing one native app-server and one synthetic account home do not prove sharing between separate app-server processes or account isolation. Decodex has not added a launch flag that enables sharing; the native user setting was not inspected. Follow [Decodex issue #2522](https://github.com/acg-box/decodex/issues/2522) and requalify the exact unmodified CLI, helpers and retained mixed-server and child-input regressions before adding a sharing default.

## TLS dependency qualification

The locked HTTPS graph selects `rustls` 0.23.46, `rustls-webpki` 0.103.16 and `webpki-root-certs` 1.0.11. Account refresh and component downloads use `reqwest` clients. The selected dependencies include certificate-distrust updates. The `workspace_dependencies` section of the [current qualification](../../automations/upstream/qualification.json) identifies the selected source, artifact checks and executed regressions. The [original TLS record](../../automations/upstream/retained-contracts.json) retains its earlier package identities and limits.

In the original macOS arm64 TLS batch, the baseline and candidate each passed a full workspace check and 36 account-login and component tests. The candidate passed 1,891 headless tests and strict lint for 11 packages. The official `rustls-webpki` source test `verify_cert::tests::distrusted_spki` executed once and passed with `ring` enabled. Earlier attempts with missing fixtures or no selected test remain separate from that pass. These checks do not qualify live HTTPS, other platforms, installation or release.

Four synthetic voice tests passed after the user handled the firewall prompt for the test executable. A later 40-execution stress run had 39 passes and one post-unmute audio-energy assertion failure. A separate interleaved comparison then passed 80 executions across the previous and candidate test executables. The isolated failure remains unexplained; see [issue #2529](https://github.com/acg-box/decodex/issues/2529). None of these results qualifies physical audio or installed-app interaction.

## WebRTC build archive

The default macOS build routes `webrtc-sys-build` 0.3.21 through a Decodex guard and selects the `webrtc-a545de4` native release. Before extraction, the guard checks the complete ZIP against the pinned official SHA-256 and checks the local and central ZIP headers for consistency. A digest or header failure stops preparation before it replaces the existing build cache. The guard compares cached regular files with the verified archive, stages repairs and preserves the upstream owner-access behavior. An existing `LK_CUSTOM_WEBRTC` directory remains a developer-provided input; the official archive checks do not qualify it.

In the original guard qualification on macOS arm64, the default build retained the actual downloaded ZIP and checked its pinned digest. Its 38,722 extracted files matched that verified archive, and the static library matched the earlier qualified library. Both official macOS arm64 and x64 ZIPs passed their pinned digest and strict-header checks; native x64 execution was not tested. Six local guard tests passed, including incorrect digest, inconsistent headers, cache repair and file permissions. The full workspace check, strict lint for 13 packages and four synthetic voice tests passed. Two voice tests each passed 20 repeated executions. Every iteration and the final summary were inspected because the observed Nextest 0.9.143 can return zero after an earlier failure with fail-fast disabled.

That original full Rust style audit reported the same 163 findings on its exact main snapshot and branch. The three new source files had no findings in their focused audit. Eleven selected upstream ZIP tests passed in the [previous delivery](https://github.com/acg-box/decodex/pull/2530); one intentionally demonstrates the earlier default behavior without strict header enforcement. The previous builder deleted its downloaded ZIP before its digest was observed. Its extracted files matched a separate verified archive, but that earlier result does not identify the deleted ZIP bytes. The [stable contract catalog](../../automations/upstream/retained-contracts.json) retains this original qualification and its limits.

The current `workspace_dependencies` qualification separately records both selected
official ZIPs, their headers and compiled trust-anchor probes, the linked arm64
library, and the signed real WebRTC transport lane. The retained public certificate
fixtures verify that the retired AAA root occurs in the qualified old library but
is absent from both new libraries, while a control root remains. The [reproducible
procedure](../operations/commands-and-validation.md#qualify-native-dependency-artifacts)
does not require an old disposable build directory. Static artifact inspection
does not execute a TURN/TLS handshake. Workspace dependency results do not migrate
the separately qualified Codex CLI or extend installed-app acceptance.

These checks do not qualify arbitrary malformed archives, unpinned default targets, physical audio, live providers, installed-app behavior or release. They do not establish the cause of the earlier [unmute assertion](https://github.com/acg-box/decodex/issues/2529).

## Signed native WebRTC tests

The separate `native-voice-test-development-signing` contract qualifies the explicit `test-native-voice` command on the measured macOS arm64 host, development certificate and stable test path. After the first user-confirmed firewall allow, the original and rebuilt test executables each passed four real WebRTC regressions. Their code hashes differed, their designated requirements matched, and both test windows contained known allow decisions without queued or prompt events. Two task reruns passed the same four tests, for 16 post-consent executions. The first pre-consent ICE timeout remains failed. Certificate, path, OS and policy changes and a locked session remain unmeasured. This result does not resolve the older post-unmute assertion or qualify physical audio, a provider or the installed app. See [the signed test procedure](../integrations/subscription-voice.md#run-native-webrtc-tests-with-a-development-signature).

## Selected read-only views

The approved views add native memory inspection, collaboration reply/contribution history and retained Agent/Work usage. Their current contract and portable proof are under `selected_read_views` in the [stable contract catalog](../../automations/upstream/retained-contracts.json). Public delivery is tracked in [PR #2508](https://github.com/acg-box/decodex/pull/2508); inspect its current head and checks separately from local qualification.

| Evidence | Qualified behavior and limits |
| --- | --- |
| Native source and schema | Official source `970111f6cb62cc02fe36a82ea7d71b37a2889c97`, selected CLI and both helpers were identified. A fresh experimental schema was generated and hashed. The upstream memory implementation and test bodies were read; upstream Rust tests were not executed in this qualification. |
| Exact-artifact memory fixture | Native version selection, disabled/missing/blank summaries and an unstarted thread were checked without fallback or model requests. The read does not resume a cold context, write memory or generate a summary. |
| Exact-artifact usage fixture | Two controlled responses across native process restart retain input 30/output 6 and two native durations without counting cumulative usage twice. This does not measure live-provider billing. |
| Local collaboration regressions | Saved local answers use exact Agent/thread/question identity. Delivery, receiving-turn completion and resolution stay distinct. Independent request repair cycles retain their own immutable instruction and assessment. Unknown external answer bodies remain unknown. |
| Local usage regressions | Exact Agent/thread/turn identities deduplicate retained totals after store reopen. Missing usage remains unknown; turns linked to several Works are excluded from Work totals. Preview limits do not limit aggregate totals. |
| GPUI regressions | Four selected tests check exact memory copy, draft preservation and binding invalidation, answered-question sources, separate repair cycles and usage labels/limits. They establish fixture interaction, not installed-app acceptance. |

The full workspace run passed 2,998 tests and skipped 1,222. It preceded one added memory copy/display test; the later four-test UI cohort includes that test. Two exact-artifact native tests, four selected UI tests and 28 Python tests passed, with zero unmatched native/UI selectors. Strict Clippy passed for five changed packages; formatting and the schema59/90-table database gate passed. The gate inventory was repaired to match the existing canonical migration ledger; no migration was added.

The full vstyle audit failed with 16,517 findings at implementation commit `1beae0c0415c28ef45d8afaa1bd8117832c62c25`. This qualification does not exempt those findings or classify them all as inherited. The independent repository-wide style cleanup retains that work. Exact selectors, entrypoints, commands and selected source digests remain in the portable index after local products are removed.

A Work result or delivery identifies a linked native turn; it does not prove that every action in the turn belongs to that Work. Input includes cached input, and output includes reasoning. Missing historical or child receipts remain unknown. Summed native duration can overlap and is not Work elapsed time. No complete Work cost, monetary allocation, installed-app interaction, real-provider acceptance, app installation or release is claimed. The selected runtime baseline, upstream review cursor and automation schedule do not change. The user later selected attached branches; Guardian V3 waits for upstream maturity and custom managed configuration is deferred. A rebuilt runtime requires new artifact/helper/schema observations and affected regressions; it is not the original deleted artifact.

## Separate-question popover qualification

The [portable separate-question record](../../automations/upstream/retained-evidence.json#quick_separate_questions) identifies source `b26ecc90d27116263afd8312ac1b8f54efe97699`, the retained UI contract, exact test names, commands and source/test-artifact digests.

The focused cohort passed 28 tests: 17 sampling and suggestion checks plus 11 rich-text selection checks, with 1,176 skipped and no unmatched selector. It covers direct toolbar and response entry, selected excerpts, draft and reading-position preservation, Escape, stale-source rejection, Unicode excerpt bounds, separate suggestion results and one-request readback. These are GPUI TestPlatform and loopback-wire checks.

That historical build used `gpui_platform/runtime_shaders` because the host lacked the Metal Toolchain. The default-build gap and the existing archive-renderer Clippy violation were resolved in [PR 2570](https://github.com/acg-box/decodex/pull/2570). The Side chat check below also passed the default build and strict GPUI Clippy. This does not turn the historical run into a default-build result.

## Side chat tab qualification

The [Side chat record](../../automations/upstream/retained-evidence.json#side_chat_tabs) binds source `84349473fc6cb1402f6962d49b86d3eeeacd097f` to exact source and artifact hashes, commands, test identities and limits. Nineteen selected GPUI checks passed, with 1,191 skipped and no unmatched selector. The checks cover confirmed attachment grouping, source changes, draft and viewport preservation, close and reopen behavior, narrow-header geometry and the attached confirmation boundary.

The default application build, strict GPUI Clippy across all targets and features, Rust formatting and whitespace checks passed. Offscreen synthetic captures cover dark, light and narrow header layouts. Each capture uses an explicit system-temporary draft root that is removed afterward. The fixture has no connected profile; its storage warning and clipped full-content capture do not qualify ordinary conversation storage or complete responsive layout. A separate GPUI assertion checks the header at a 650-point viewport.

This presentation reuses the retained attached-fork runtime qualification. No native or upstream test was executed for this batch, and no runtime artifact, helper, schema or review cursor changed. The selected upstream protocol, processor and two attached-thread test bodies were read. Neither these reads nor the offscreen captures establish live-provider, physical-keyboard, installed-app, latency or performance acceptance. No application installation or release was performed.

## Current desktop feedback qualification

`desktop_feedback_scope` in the [portable evidence index](../../automations/upstream/retained-evidence.json) qualifies the selected conversation changes at source `cfa54c6f2689cf5b2f4e1f09afbcc1664396eb31`. The cohort executed 25 distinct Decodex tests: 19 GPUI and six runtime projection tests, with no unmatched selector. Four upstream test functions were read; no upstream suite was executed. A function-level comparison with the original dispatcher reproduces the reconnect waiting failure, and the repaired source passes with the original key, pending-approval pause and later-edit preservation.

The 25 test assertions passed. One pure layout-state case received a nextest process-cleanup marker in the combined run; its exact isolated repeat passed without the marker. The exit-delay cause remains unmeasured. Four affected Rust packages passed repository Clippy deny flags with all features and targets. Nightly format, 14 architecture/gate tests and the menu-bar Swift build passed.

The selected native source remains `33d306d457a4632b22bebf214e0c25d91f90f6e8`. The CLI, Code Mode helper and `rg` byte digests still match the earlier qualification. The previously generated exact-artifact schema document passed its canonical digest check and contains the nullable model/effort metadata. This work made no runtime migration or provider request. An offscreen native workspace-loading capture shows the cloud mark. Installed interactions, physical display timing, native child-window focus and packaged SwiftUI rendering remain unmeasured. Find covers loaded retained text and explicit earlier pages; shortened output remains incomplete.

The standalone Codex startup-configuration diagnostic capability and its dedicated helper are retired. Historical executions of that control remain evidence at their original source revision and create no current validation requirement. Decodex product/integration diagnostics and normal warning presentation remain supported. The source-review cursor and paused automation are unchanged.

## Conversation-list branch qualification

The list entry is delivered in [PR #2559](https://github.com/acg-box/decodex/pull/2559). The `conversation_list_branch` entry in the [portable evidence index](../../automations/upstream/retained-evidence.json) binds source `5f2fbffac0a5590affd76dcdc3bc73ff83b3cefe` to eight executed GPUI tests with zero unmatched selectors. They check the latest completed turn, its own user input, exact source identity, unmanaged-row refusal, draft preservation, cancellation and the existing explicit native branch confirmation without sending input. Strict GPUI Clippy and nightly formatting passed.

This change adds a Decodex entry and reuses the retained native fork contract within its recorded limits. Relevant upstream protocol and implementation were read; the active-turn rejection test was inspected only in part. No upstream tests or exact-artifact native tests ran in this batch. The runtime selection and source-review cursor did not change. Physical keyboard behavior, app startup, installed-app and live-provider acceptance remain unmeasured. No installation, release or schedule activation occurred.

## List status filter qualification

The `status_list_filters` entry in the [portable evidence index](../../automations/upstream/retained-evidence.json) binds source `5c95cf7a8c283369e14b6de9c171d3bc563bbb9c` to five executed GPUI checks with zero unmatched selectors. Three click-through fixtures check combined status, Project and search filtering, changed native status labels, separate Work/conversation choices, exact row opening and retained drafts. Two existing browser regressions check preview reuse and opening closed work without losing other drafts. Strict GPUI Clippy with all features and targets, nightly formatting and whitespace checks passed.

The fixtures use GPUI TestPlatform and synthetic snapshots. They do not start a service or native process or write host preferences. Production preference persistence reuses the existing macOS NSUserDefaults owner; a physical application restart and installed UI were not exercised. No native artifact, upstream test, live provider, installation, release or schedule activation is qualified by this presentation change. The source-review cursor and selected runtime remain unchanged.

## Native history, resources and estimate qualification

The `history_resource_cost` entry in the [portable evidence index](../../automations/upstream/retained-evidence.json) retains the exact source, commands, test identities, selected component/helper digests and qualification limits for items 35, 43, 67 and 72. It separates 19 distinct qualified Rust checks, two Menu Bar tests and 14 existing architecture/gate checks from four upstream test bodies read but not executed. Strict Clippy for the five affected packages and nightly formatting passed. The prior cohort retains its source and behavior limits. One added UI regression first reproduced an unavailable navigation action, then passed with exact current-thread admission and reference copying; strict GPUI Clippy passed again for this repair.

One Rust check runs the exact selected native artifact in a system-temporary private store with two synthetic Responses turns. It checks an exact same-turn history anchor, matching registered resource owners, archived filtering and an empty unmatched lookup. The lookup makes no additional model request. A reusable repository fixture preserves this proof after local logs are removed. Socket and GPUI checks separately cover opaque continuations, source changes, selected-child identity and collapsed output counts.

The selected native source remains `33d306d457a4632b22bebf214e0c25d91f90f6e8`. CLI and helper digests were read back, and its experimental schema was generated again. This is no runtime migration or source cursor advance. The tests do not establish live provider billing, full account usage, cross-store discovery, historical upload search or packaged Menu Bar interaction. Installed-app acceptance stays separate. No application was installed or released, and automation stays paused.

## Native item navigation presentation repair

The `history_anchor_navigation` entry in the [portable evidence index](../../automations/upstream/retained-evidence.json) binds source `af989d83632b50adc83c420ddc60778cfd7b0885` to 19 executed GPUI checks: six item-navigation cases, four existing Find cases and nine viewport cases. All passed, 1134 tests were skipped, and each of the three selectors matched. The ordinary-message button regression first failed on the completed main baseline with only the new test overlaid, then passed after repair. Strict GPUI Clippy with all features and targets, nightly format and whitespace checks passed.

Rendered and same-UID wire fixtures cover direct loaded-item location and temporary unfolding, exact unloaded turn/item reads in the main transcript, native continuation cursors, failed-page retention, a fixed return action, the prior scroll offset, retained drafts, selected-child identity and late-source rejection. The reusable cases are in `apps/decodex-gpui/src/agent_item_context_tests.rs`. Item pages remain bounded and separate from canonical timeline ordinals. Find and Latest return to the ordinary conversation.

This is a presentation repair of the existing item reader. The selected native runtime, helpers and schema are unchanged, and their earlier qualification retains its original limits. One upstream paging test body was read; no upstream or native artifact test was executed for this repair. Installed-app, live-account, physical display, provider and performance acceptance remain unmeasured. No installation, release, review-cursor advance or schedule activation is implied.

## Persistent attached branches

`retained_attached_forks` in the [stable contract catalog](../../automations/upstream/retained-contracts.json) records qualification against native source `970111f6cb62cc02fe36a82ea7d71b37a2889c97`, with separate CLI, Code Mode helper, `rg`, source archive and generated-schema digests. Local protocol is 3.2; the database remains schema59. The two relevant upstream test functions, containing five cases, were read but were not executed.

Five exact-artifact native tests passed: the public-socket fork workflow, source-provider fork control, archive roundtrip, parallel Code Mode tool control and startup configuration control. The startup-configuration control is now retired; its execution remains a historical receipt. The fork control exercises legacy and paginated native history, Goal exclusion, provider preservation, restart, archive/restore and refusal to delete a parent with attached children. The public-socket case creates nine source turns and checks all three explicit branch choices, saved-choice retries, retained exact-source listing and canonical draft recovery. Creating or discovering a branch makes no model request. A separate explicit send adds one request to the attached branch and leaves the source history unchanged.

Current compatibility keeps the original source boundary as provenance but validates an empty child-visible history for attached creation. Native inference retains the source context, subsequent child turns are independent, and the original history is unchanged. Attachment requires paginated source history. The raw legacy negative control and ordinary legacy fork controls remain explicit. These current checks supersede the historical child-prefix expectation without extending its installed-app scope.

The focused local cohorts passed 57 backend tests and six UI tests with zero unmatched selectors. The complete workspace run passed 3006 tests with 1227 skipped and one passed-test LEAK in the streamed-draft composer test. That test then passed cleanly in the focused UI cohort. The final affected checks also passed strict Clippy for five packages, nightly formatting, 28 Python tests, the schema59 database gate and a 1076-file style audit with zero findings. The complete workspace run preceded later test-fixture diagnostics and formatting; final affected and native checks followed those changes. These cohorts overlap and do not explain the earlier cleanup marker.

The native provider is synthetic. Transport fixtures cover uncertain creation and recovery; no physical crash was injected. The newest-100 query bound was reviewed in source without a 101-record stress run. `rg` integrity was verified but this cohort did not invoke it. External native branch adoption, automatic follow-up, scheduling, result merge and deletion controls are outside this feature. Archived branches retain native restoration. No installed desktop interaction or real-account acceptance was performed. No installation, release, runtime migration, review-cursor advance or schedule change is implied. Use the [portable runner](../operations/commands-and-validation.md#attached-branch-native-regression-fixtures) to reconstruct affected proof after local cleanup.

## Historical receipts

Past experiments remain in Git history with their original revision and limitations. Do not repeat a dated test count as current validation or turn an old unresolved experiment into a new requirement. New acceptance reports must identify the checked artifact, inputs, observed outcome and remaining uncertainty.

The embedded MCP HTML viewer and local plugin management are retired. Their old acceptance records do not require reintroducing those features. Voice still needs separate physical capture, subscription transport and transcript checks. See [current scope](../decisions/upstream-product-scope.md) and [subscription voice](../integrations/subscription-voice.md).

## Proportionate validation

Choose the smallest test that can fail for the changed behavior. Preserve complete failure output and the actual artifact identity. Do not rerun long desktop or physical-provider flows to remove every historical open statement. Use isolated, task-owned fixtures that satisfy the runtime path rules, and remove owned test processes and data when done. Documentation consolidation alone does not justify a fresh model call, application launch or complete Rust rebuild.

## Source extraction and build caches

`node --test scripts/macos/build_codex_source.test.mts` checks official source selection and extracts an archive with old timestamps into an existing source cache. The extraction test verifies that obsolete files are removed, archive content is restored, and file timestamps become newer than a simulated cached-build time. This checks the extractor contract. Qualify the resulting package, generated schema and affected native behavior separately.

## Component migration

`decodex-components` tests cover exact official source selection, release baseline coverage, download interruption and retry, hash refusal, atomic replacement, failed publication preservation and damaged cache handling. The ignored official package test uses an isolated store to exercise failed network access, retry, offline reuse and schema repair with a real signed Release. That fixture is not the production pin.

The ignored development test takes a qualified package through `DECODEX_TEST_DEVELOPMENT_RUNTIME`. It verifies exact executable bytes after import, repairs damaged executable and schema files from the local archive, rejects a different commit and keeps the previous selection after an import failure. Run the affected native consumer fixtures against the component-store executable as a separate check. Matching hashes can carry forward evidence for the same artifact; matching a version string or schema alone cannot.

During a component migration, record whether a failed fixture reached its behavior assertions. Preserve full failure output and each exact executable identity. Compare the previous and proposed components under the same fixture limits. Resolve startup failures before reporting the affected behavior as qualified; a shared initialization timeout does not establish its cause.

Formal release selection must retain the currently reviewed fixes. The pin tool rejects an official Release that does not contain the development baseline; reviewers must also establish affected behavior and schema compatibility. No test fixture or successful package download authorizes a lower baseline.

The signed stage test checks that the app contains the runtime requirement and Sparkle, excludes bundled Codex executables and schemas, and rejects a missing production pin before changing an existing stage. It does not install the app, prove a public update feed, or establish live provider and physical audio acceptance.

## Source fixture scope

The [portable qualification record](../../automations/upstream/qualification.json) identifies exact source tests, artifact and helper hashes, reusable probes and limits. The [stable contract catalog](../../automations/upstream/retained-contracts.json) identifies retained consumer requirements and original feature limits. The [current router](../../automations/upstream/retained-evidence.json) links the selected qualification. Native run receipts retain raw output; Git retains replaced compact records.

The source cohort uses `dev-small` helpers. Nextest can replace requested `CARGO_BIN_EXE_*` paths with source build outputs. Only the separate Decodex native cohort and standalone probes establish exact production CLI behavior. The direct signed Code Mode helper test bypasses Nextest path replacement. The qualification record preserves these roles and the original assertions. Older source fixtures that require private startup arguments retain their own launcher and build limits in Git history. Passing a source fixture does not establish behavior through a production API. Test supported production inputs against the exact selected CLI and helpers as a separate step.

Earlier conditional remote-credential snapshot cases failed because the host resolver returned a reserved address or could not resolve a fixture hostname. Native address protection remained enabled. Current Decodex runtime consumers use app-server stdio and do not provision a remote executor. The record qualifies retained consumers within that scope; it does not qualify the failed conditional remote behavior. Reopen that qualification only when a concrete retained consumer or required retained-fix proof makes it applicable. A fixture environment becoming available alone does not create daily maintenance work. Preserve the failure evidence, original assertions and native policy.

## Terminal errors and external context

The terminal projection retains the native turn status and bounds its error details. Capacity retry requires an exact failed-turn readback with `serverOverloaded`. The `quota_other_errors_and_missing_history_do_not_schedule_capacity_retries` test includes `other` and `policyCapacityUnavailable`; neither selects an overload retry. The `failed_turn_keeps_public_reason_and_explicit_bounds` test checks the public failure reason and its size and privacy limits.

The exact-artifact `model_errors.py` probe covers HTTP 404, SSE `error` and SSE `response.failed` carrying `model_not_found`. Each candidate case makes one request without retry and preserves `modelNotFound` plus the provider message in live, stored and cold history. The native capacity fixture also checks that Decodex does not schedule another attempt. Source tests cover additional HTTP and WebSocket mappings; these source cases are not additional artifact transport executions.

The same model-error probe checks `policy_capacity_unavailable` over HTTP 503 and SSE `response.failed`. Native `policyCapacityUnavailable` and the provider message survive live, stored and cold reads. The HTTP fixture permits three native requests with `request_max_retries=2`; the SSE case permits one. Native transport retry is distinct from a Decodex continuation, and no Decodex automatic retry is added. Bare SSE `error` policy classification is outside the upstream change and this qualification.

`large_catalog.py` checks native discovery from an explicitly configured synthetic catalog larger than 1 MiB. `response_identity.py` checks that a provider message without an ID keeps one native identity through live events, storage and cold read. The identity probe passes on both the previous and candidate artifacts; it does not establish a reproduced defect in the previous artifact.

`inject_external_context` sends a `function_call_output` through `thread/inject_items`. The `uncertain_context_injection_or_following_turn_is_not_replayed_after_restart` test checks a lost injection response and a lost follow-up response. Recovery keeps the outcome unknown and does not replay either request.

These local tests use synthetic transport. For a runtime change to incomplete responses or final recording, also qualify the selected native runtime. Count provider requests and inspect the terminal status for ordinary sampling and compaction. Cover SSE and WebSocket responses, and keep interrupted-response and content-filter controls separate. Use the upstream regression test to check input that arrives while a turn records its final batch. Keep these results separate from installed-app acceptance.

Keep reported spending and current context separate. The Agent stores cumulative token counters and derives turn usage from their differences. Context comes from the native last-request counters. When an incomplete response reports usage, check that spending increases without replacing the context baseline or causing another compaction. Missing or malformed usage must not become an estimated charge. Compare the previous component and the proposed component with the same fixture.

## Failed compaction and cold resume

`installed_native_discards_partial_compaction_output_after_restart` selects an absolute CLI path through `DECODEX_TEST_CODEX_BINARY`. Its loopback Responses provider uses the local compaction path. The provider sends a partial assistant summary and then `response.incomplete`. After the failed turn, the fixture restarts app-server and retries compaction. It checks that the retry retains the original assistant evidence and excludes the failed summary from durable history.

Run the same fixture against the previous component and the proposed component, and record each executable hash with the result:

```sh
DECODEX_TEST_CODEX_BINARY="$QUALIFIED_CODEX_BINARY" cargo +stable nextest run \
  --locked --all-features -p decodex-runtime --lib --run-ignored only \
  -E 'test(installed_native_discards_partial_compaction_output_after_restart)'
```

The fixture is ignored by default. It checks the named CLI with synthetic input and a system-temporary home. The separate compaction fixtures check successful checkpoint continuation and preservation of the incoming prompt after a failure. These checks do not establish all provider paths, installed-app acceptance or a release. Keep dated execution and artifact receipts in the native automation directory.

## Saved settings and diagnostic storage

The reusable `thread_settings.py` probe selects a service tier and reasoning summary through native `thread/settings/update`. It completes one turn, closes the native process, changes the global defaults and resumes the same task without overrides. It checks the resumed tier and the next provider request in both legacy and paginated history. Each case permits exactly two inference requests, so recovery cannot hide the result by replaying a turn.

Two further settings cases inject stale turn context after an acknowledged permission update. Fork and cold resume retain the acknowledged on-request policy, automatic reviewer and read-only permissions in both history modes, with one inference request each. A separate unchanged-settings case checks that cold resume adds no checkpoint. The startup probe checks that both a missing `.env` and a directory at `.env` exit `--version` promptly.

The `log_storage.py` probe checks the default diagnostic database, an explicit `log_dir`, and an environment-selected state directory with a separate diagnostic directory. A fourth case verifies that an unavailable diagnostic directory does not prevent unauthenticated `login status`. These cases use system-temporary homes. Managed storage precedence is covered by the recorded source tests; this probe does not qualify NFS or migrate existing databases.

Set `QUALIFIED_CODEX_BINARY`, `QUALIFIED_CODEX_SOURCE_COMMIT` and `DECODEX_QUALIFICATION_OUTPUT` as described in [Commands and validation](../operations/commands-and-validation.md#qualify-retained-native-behavior), then run:

```sh
python3 crates/decodex-runtime/tests/native_qualification/thread_settings.py
python3 crates/decodex-runtime/tests/native_qualification/log_storage.py
python3 crates/decodex-runtime/tests/native_qualification/dotenv_startup.py
```

The [qualification record](../../automations/upstream/qualification.json) binds each probe to the exact CLI and packaged helpers. The [earlier storage before/after receipt](https://github.com/acg-box/decodex/blob/f6640838f0997493704ff87e46205b46a700eef2/automations/upstream/qualification.json) retains its original artifact scope; the current record contains the selected artifact's requalification. The checked-in probe implementations remain available after local build products are removed. A rebuilt artifact requires its own identity and qualification; these results do not establish installed-app or real-provider acceptance.

## Context defaults and selected retention

Settings tests cover the shared configuration contract, not a separate local compaction engine:

| Check owner | Contract |
| --- | --- |
| `context_preferences_tests` | A threshold-only async table does not enable the feature; only a versioned boolean edit for the two supported preferences is admitted |
| `context_settings_bind_source_version_and_report_uncertain_readback` | A changed source or stale review cannot write; async tables retain their threshold; saved user off remains distinct from an effective project override; failed readback does not trigger another write |
| `context_picker_sends_once_then_reads_effective_override_after_lost_reply` | A GPUI click dispatches once; the lost reply leads to a settings read and uncertain feedback instead of automatic replay |
| `installed_native_context_preferences_share_config_and_preserve_thresholds` | The named native binary, production launch arguments and an isolated home share one config file; explicit off, boolean/table forms, threshold preservation and stale manual-edit refusal are checked |

The last fixture requires `DECODEX_TEST_CODEX_BINARY` and is ignored by default. Its name does not establish installed-app acceptance. It reads and writes only its system-temporary configuration. The preferences apply to new conversations; loaded conversations keep their settings. No configuration fixture establishes a real-provider speed or cost benefit.

`delegated_instructions_keep_tool_authority_on_creation_and_followup` checks that initial delegation and explicit follow-up use empty user input and a `work_instruction` tool output with `retain: true`. Direct user input keeps its user role. The dispatch encoder leaves wakes, capacity retries and other external results unselected. The uncertain-delegation and injection tests check that lost results are not replayed as user input after recovery.

For native retention qualification, use the exact selected artifact and include disabled-feature and unselected-output controls, compaction, cold resume, tool provenance and duplicate-insertion checks. Native configuration, eligibility and budget still own preservation. For background compaction, also check reuse of prepared work, newer suffixes, cancellation and cold recovery. Keep these native behavior results separate from local configuration tests and real-provider latency or request-cost measurements. Record dated results and artifact identities in the native automation directory.

## Native branch and credential fixtures

The ignored local-socket fixture can select native branch qualification with `DECODEX_TEST_NATIVE_FORK`. It requires an explicitly selected native binary, matching isolated `HOME` and `DECODEX_TEST_ACCOUNT_HOME`, and its fixture marker. The provider and accounts are synthetic. It checks an empty branch before the first input, an inclusive branch after that turn, preserved source history, distinct native identities, repeated-command readback and canonical draft handback. It asserts that branch creation and recovery do not add model requests.

For the ordinary Conversation selected-directory path, place the selected workspace under the operating-system user's home, outside Codex-owned roots. Use an existing task-owned workspace for this boundary and keep disposable account homes and other fixture data in system-temporary directories. Overriding `HOME` does not change the operating-system account home used by the selected-working-directory check. A fixture under `/tmp` can therefore fail before creating a conversation. Direct native read-state and prediction fixtures use system-temporary directories and do not invoke that ordinary Conversation directory selector. Keep local socket paths short enough for the platform. Do not loosen product path checks to make a test run.

PAT tests separately cover versioned import, native identity hydration, optional OAuth-only fields, exact child environment binding and shared-auth Route. Synthetic credentials do not prove the entitlement or validity of a production PAT. Never use production account data to repair a fixture setup failure.

## Native additions and limits

The ignored read-state fixture checks explicit read/unread marks, stale-revision refusal, persistence after a cold reopen and unavailable receipts for ephemeral threads. Reading metadata does not acknowledge it or make a model request.

The ignored prediction fixture checks parent context and tool-catalog inheritance, cancellation before inference, cleanup, absence from durable history and unchanged parent metadata. Prediction inherits tools and permissions; it is not the isolated recap path. This capability does not add automatic suggestions or a new product entrypoint.

For a runtime change to prediction compaction, also test a fork above the compaction threshold. Check pre-turn, tool-continuation and post-turn boundaries in both native history modes. The prediction must preserve its inherited context while the parent retains normal compaction. Check context-limit failures and model-requested context resets separately. The local inheritance fixture alone does not cover these cases.

The native launch fixture checks admitted Code Mode launch arguments, child context defaults, Agent message wakeups and fork-prefix preservation while retaining boolean and table forms of the user's multi-agent selection. Semantic prefix preservation does not prove provider cache savings. Synthetic context-limit and compaction probes do not prove real-provider latency or cost improvements. Code Mode transport remains controlled by native configuration; this integration adds no transport selector or universal tool-output retention policy.

## Question routing, completion and cancellation

The [retained coordination evidence](../../automations/upstream/retained-contracts.json) links the bounded historical real-model evaluations for self-resolution, peer coordination, parent escalation, user decisions and persistent Agent completion. Those evaluations cover the legacy managed-worker contract for exact inbox receipts, separate child-goal resolution and an idle-only Busy refusal. In that legacy continuation path, Busy means the instruction was not sent or queued, so the caller waits for the same worker. The new default uses native Subagents; independent Collaboration Requests have their own durable busy queue and acceptance contract. Do not apply the legacy Busy refusal as the current peer-queue behavior. The historical evaluations used one model and effort with controlled scenarios and predefined user choices. Current synthetic child-approval and local library regressions are separate requalification; they do not repeat real-model judgment or establish every model, permission configuration, race or installed interaction.

The [sampling qualification](../../automations/upstream/qualification.json) separately records explicit cancellation of Ask separately. Closing its panel only hides presentation; Cancel answer targets the exact native sampling request. A cancellation request, native acknowledgement and a result that wins a race are different outcomes. Preserve exact request ownership and uncertain-state readback. This is not cancellation of the parent conversation or a general scheduler. These receipts do not establish installed-app acceptance.

These fixture definitions specify what can be checked. They do not claim a new execution result from this documentation refresh, installed-app acceptance, or release delivery.

## Project, Agent, Work and global Guardian

The `retained_project_agent_work` entry in the [stable contract catalog](../../automations/upstream/retained-contracts.json) is the reconstruction authority for this change. It records contracts, current source symbols and digests, tests read separately from tests executed, exact selectors/entrypoints, artifact/helper/schema identities and public delivery. Detailed raw receipts stay in the native automation directory. Missing old local products do not erase certified earlier ranges; a rebuilt artifact needs its own identity and qualification.

The original Project, Agent and Work qualification used official source `970111f6cb62cc02fe36a82ea7d71b37a2889c97`; current runtime qualification is separate. The package passed explicit `cargo +stable make prepare-codex` component, helper and schema verification. It did not replace the installed application or advance the upstream source-review cursor.

The following table preserves the original architecture qualification and its separate media follow-up.

| Executed check | Recorded outcome |
| --- | --- |
| Workspace check, strict lint, format | Passed; lint covered 13 packages. |
| Database suite and schema gate | 188 passed; schema 56, 89 tables and integrity checks passed. |
| Exact packaged native matrix | 13 passed; 985 excluded; zero unmatched selected tests. |
| Transport/current Work receipts | 28 passed; 970 excluded. |
| Focused rendered UI and runtime | 6 passed. |
| Python and CLI diagnostics | 40 and 2 passed. |
| Original complete workspace/all-targets/all-features | 2918 executed: 2916 passed, 2 failed, 1154 skipped. One passed test had a LEAK marker. |
| Later current-main complete workspace | 2918 executed and passed, 1154 skipped, no LEAK. |
| Later repeated native media pair | 40 executions passed per source on current main and the pre-change baseline; zero retries and unmatched selectors. |

Counts overlap. The original two full-suite failures are `native_call_exchanges_audio_captions_mutes_and_releases_buffers` and `remote_channel_loss_reports_failure_and_releases_buffers`, both native ICE connection timeouts. Their failed receipt remains intact. The later `native_media_followup` compares identical audio source, dependency and WebRTC feature identities across current main and the pre-change baseline. Historical macOS firewall traces queued inbound flows without a verdict during both architecture failures; later passing runs had known allow verdicts. This supports an external firewall admission cause, with no identified architecture-induced audio change. Recovered session events place both failures within the locked interval. The already-allowed package-only baseline passed in that same interval. The workspace binary received its prompt and known allow verdict after unlock. A new controlled lock or firewall-denial comparison was not run. See the [native media diagnosis](../integrations/subscription-voice.md#diagnose-native-media-ice-timeouts). The original leaky pass was `ui_working::tests::appearance_and_disappearance_share_one_continuous_fade`; its cause remains unknown. No product source, test assertion, deadline or runner policy changed.

The native matrix covers production-bridge Project idempotency, reviewer routing and private observations; V1 helper attribution; V2 idle-owner wake with both feature settings; child approval and MCP identity/policy; saved/live native permissions; failed compaction and restart; large Guardian actions; Code Mode yields and parallel dynamic-tool order. The reusable Python `project_guardian.py` adds a synthetic exact-CLI Project/private-review probe. It uses `QUALIFIED_CODEX_BINARY` and `DECODEX_QUALIFICATION_OUTPUT` as described in the [command guide](../operations/commands-and-validation.md#qualify-retained-native-behavior).

Local regressions cover one Agent across four Works in two Projects, wrong-Work Result refusal, current-requirement guards, requirement changes, scoped Collaboration/repair/cancellation, exact command retry, lost delivery, source/digest verification, paused Work, restart and selected evidence paging. The SQL scope defect was reproduced before repair: unrelated earlier records hid the Agent's own Work. The repaired test also verifies that only granted Materials enter a bounded peer read.

### Native Project and nested Work follow-up

The portable `native_project_and_work_followup` entry binds the repairs to the same source, CLI, helpers and generated schema. Before-repair tests reproduced unassigned retained Project membership and missing Work attribution for permitted V2 nesting. V1 nested attribution passed unchanged.

| Executed check | Follow-up result |
| --- | --- |
| Exact-artifact native cohort | 7 passed, 996 excluded, zero unmatched selectors. |
| Database suite and gate | 189 passed; schema 57, 90 tables and integrity checks passed. |
| Focused Work/identity guards | 4 passed, 999 excluded. |
| Workspace check, strict lint, format | Passed; lint covered 13 packages; Nextest TOML format also passed. |
| Python and CLI diagnostics | 40 and 2 passed. |
| Same UI artifact in isolation | Five passes per affected sample, ten executions in total. |
| Final complete workspace | 2921 passed, 1157 skipped, no LEAK. |

Counts overlap. The native Project test reuses the catalog, attaches a retained unassigned conversation, restarts the actual app-server and verifies unchanged conversation, directory and permissions. It preserves an externally conflicting Project and proves an independent queued Agent still runs. This qualifies native data and membership; official desktop saved-Project/sidebar presentation remains unmeasured.

The nested V2 test uses native configuration that explicitly permits Code Mode collaboration. It saves real native child input before binding, reconstructs the durable point before parent attribution and reopens the store. `work_read` recovers from retained input and actual history without extra model or spawn requests. This is a store-reopen reconstruction, not a physical machine crash. V1 nested and existing direct paths are separate native cases.

The first full follow-up run had three failures and a passed resume-test LEAK. The two UI source files matched main, and the same executable passed ten isolated runs. Existing Nextest resource reservation now covers those short-frame samples with their assertions and deadlines intact. The third failure was an old fixture missing native `projectId`, corrected to match the actual response. The first LEAK cause remains unknown. Its receipt stays distinct from the clean final run.

### Source-built real-provider acceptance

The `live_acceptance` entry under `retained_project_agent_work` records the later lifecycle repairs. Its source-built bundle identifies the clean committed source `3cd19db77a8c68d141a155e65e7860e6dd598e0f`. The native runtime remains `970111f6cb62cc02fe36a82ea7d71b37a2889c97`; its CLI, both helpers and freshly generated schema match the retained identities. This acceptance used `gpt-6-astra` at Medium, without Ultra, in isolated product and native data.

| Executed check | Observed result |
| --- | --- |
| Two Projects and four Works | Two independent Agents each owned two Works. Discussion preceded assignment. All four completed under their current requirements. |
| Change and peer repair | Market attendance changed from 60 to 80. The same request received a second instruction. Mandatory cost was 1160, optional transport 160, combined cost 1320 against a 1200 budget. The trip stayed at 240. |
| Results and restart | Fourteen Results were retained; all twelve file-backed Result digests matched. Agent/native identities, current Work states, the recovered binding, Results and Guardian details survived service restart. |
| Native helper recovery | The new prepared native task name recovered its existing child and parent activity without another spawn. Later inspection recovered the native owners and exact encrypted delivery of the two old helpers; their formal Work bindings remain unconfirmed. |
| Global Guardian | A pending Ask Me approval survived a switch to Automatic Review and required its original human response. Automatic Review then executed authorized harmless probes. Five private output records were retained, including a probe on the clean committed source bundle after restart. |
| Source-built interface | Work, requirements, same-request repair, Material and Result file reveals, global settings and selected private identities were opened. This was a source bundle, not the installed application. |
| Final repository checks | 2924 workspace tests passed, 1159 skipped, no LEAK. Nine exact-artifact native tests passed with 998 skipped and zero unmatched selectors. Strict lint passed for 13 packages; format and schema59 integrity checks passed. |

Counts overlap. An earlier complete run passed its assertions with two LEAK markers in unchanged tests; both passed isolated controls. The later clean run does not establish the cause of those earlier markers.

The [reusable scenario](../../crates/decodex-runtime/tests/fixtures/project_work_acceptance.json) retains input tables, discussion and assignment prompts, the change request and expected arithmetic. Detailed receipts stay in the native automation directory. Rebuilds need their own component identity and qualification; they are not the original artifact.

This was one controlled real-provider scenario, not general model-quality acceptance. Native Project API identity and membership passed separately. Official Codex Project presentation remains unobserved: the available UI tool denies access, and the user saw the isolated official app at onboarding. No installation, release, controlled lock-screen test or audible acceptance was performed.

The later `controls_followup` record adds the following live checks without changing the four original Works or their Result records:

| Control | Observed result |
| --- | --- |
| Pause and restart | The actual Pause Work button set a user hold. It remained visible after service and UI restart. A real Agent could not prepare a helper or complete the Work with valid Result evidence. |
| Resume | The actual Resume Work button released the hold. The owner then completed only the disposable control Work. |
| Full Access | The same write outside the Project failed under Workspace permissions, succeeded under Full Access, succeeded after restart without an override, and failed after Workspace permissions were restored. Global Guardian preference remained separate. |
| Old encrypted helpers | Native parentage and identical encrypted send/receive payloads identify the venue and lodging children. Their own returned messages name the expected contributions. The exact prepared plaintext remains unproved, so no strong Work binding was written. |

The follow-up passed three selected native permission tests, one selected database hold test, strict lint for 13 packages and formatting. It did not rerun the earlier full workspace suite. An initial missing binary environment variable and an outdated per-thread reviewer fixture failed separately. That historical fixture correction set global Automatic Review and preserved its original permission, directory and request-count assertions. Current fixtures use conversation-owned permissions and keep optional review records separate. Cold profile controls can remain unavailable until the native context loads; this does not authorize an implicit resume. See the exact selectors, artifact references and limits in the [stable contract catalog](../../automations/upstream/retained-contracts.json).

### Local hardware capture follow-up

The later `media_hardware_followup` record uses the unchanged product source at `2da218b63ef39fc6dc98e518dff975852bd78929`. Three existing opt-in tests passed with an authorized physical USB microphone: deferred Live capture, repeated capture across dictation and Live modes with timely release, and native PCM delivery to a local peer across dictation. The two synthetic ICE/media tests passed separately. Each selected test ran once with no retry; the cohorts skipped 1018 and 1019 tests respectively. They are separate from earlier full-suite counts.

No microphone audio was saved or uploaded, and no system route was changed. The output route was virtual, so physical-speaker output, speech intelligibility and audible quality remain unmeasured. No controlled lock-screen or firewall-denial test was performed. Provider entitlement, a live provider voice session and installed-app behavior remain separate. See [local hardware commands](../integrations/subscription-voice.md#authorized-local-hardware-checks).

The temporary acceptance directory, test application copies, authentication copies and exact-owned helper processes were removed after the accepted database, native source history and file digests were preserved in private receipts. The user's installed applications and unrelated processes were not changed. No product or test code changed for this follow-up; no installation or release occurred.

### Installed development acceptance

The separate `installed_acceptance` entry qualifies the signed development application built from clean source `f84ba26f415d589a7b7c8cb6c7aedb4563635162`. It records application and service digests separately from the unchanged selected Codex CLI, helpers and schema. Later evidence-only commits do not change that artifact identity. This local installation was authorized separately; it was not a public release.

| Contract | Observed result |
| --- | --- |
| Business lifecycle | Two independent Agents completed four Works across two Projects, with six Materials, eight Results and three prepared native contributions. Earlier Results survived changed requirements, same-request repair and current owner assessment. |
| Conversation ownership | Two original UI defects were reproduced and repaired. Independent Agents retain composer and approval controls; input targets the selected Agent. Final typed input received the second Agent's reply, and its actual Allow once click produced the exact test file. |
| Controls and restart | Actual Pause/Resume, held preparation and valid-evidence completion refusal, installed-app restart, global Guardian and the Workspace/Full Access write sequence passed. |
| Project presentation | Decodex displayed both Projects and four completed Works. Read-only native Project and thread calls verified identities and membership without inference. Official Codex desktop Project presentation remains unobserved. |
| Saved user data and cleanup | Normal replacement and restarts preserved the measured core rows, held input dispositions, draft bytes and configuration. Test processes ended normally; temporary data, authentication copies, stages and Materials were removed after private evidence was saved. Recoverable app and data backups remain private. |

The final UI package passed 1025 tests with 1031 skipped and no unmatched selector. Its complete runs also reported cleanup markers in three unchanged tests; all three isolated controls passed without a marker. Their cause remains unknown. Twelve selected exact-artifact native regressions passed with 995 skipped and zero unmatched selectors on the recorded runtime-consumer baseline. Strict lint for 13 packages and formatting passed. The earlier 2924-test workspace receipt remains separate. No assertion, deadline or runner policy was weakened.

This is one bounded real-provider scenario, not general model-quality acceptance. Data readback does not certify every preference or window position. The user removed controlled lock-screen testing and declined another official login. No microphone audio was uploaded. That installed run did not qualify audible output or live provider voice. The later synthetic online checks below have their own limits. The old encrypted prepared-plaintext/Work-binding gaps remain unmeasured. Exact test identities, reusable fixture, source digests and reproducible commands remain available in the retained index after temporary products are removed.

### Synthetic online voice and audible playback

The separate `online_voice_acceptance` entry binds the ignored tests at source `4497c626c` to the unchanged installed service and selected native artifact. The final run used fresh product and native homes. Explicit preparation reached Ready, and actual CLI, Code Mode helper and search-helper bytes matched the retained identities. The production transport body was unchanged; only a test module was added.

Both online tests passed with 1023 skipped and no unmatched selector, retry or LEAK. Subscription dictation returned the expected synthetic words without changing Agent history. Native WebRTC returned final assistant-role text and nonzero decoded audio, and explicit Stop reached Ended. The final fixture had no open call, active Agent turn or Work. Strict lint passed for 13 packages and formatting passed. The [input generator](../../scripts/macos/prepare_voice_acceptance.py) and [test implementation](../../apps/decodex-gpui/src/native_voice_online_tests.rs) retain the reusable fixture; see the [entrypoint and prerequisites](../operations/commands-and-validation.md#synthetic-online-voice-acceptance).

The user separately heard an actual reply played through the current output route and reported low volume. The route used a virtual bridge. The test player used 0.5 and then 1.0 gain, while system volume and routing stayed unchanged. This observes audibility; it does not identify every downstream device or qualify broad loudness, intelligibility, echo cancellation or the installed UI's microphone-to-device-ADM path. No real microphone audio was saved or uploaded. One tested connection is not an entitlement guarantee for other accounts.

Temporary homes, credential copies, component copies and generated audio were removed after private domain/history records and digests were saved. One initial synthetic trial in the normal native home is confirmed archived, with its inactive history kept under native ownership. The final isolated run is separate proof. Official Codex Project presentation remains unobserved, and controlled lock-screen testing remains excluded. This follow-up needed no production voice repair, installation or release. It does not explain earlier complete-suite cleanup markers.

### Qualified native and presentation limits

The native backend comes from model catalog metadata plus configuration. The matrix uses V2 `gpt-5.6-sol` and V1 `gpt-5.6-luna` at Medium. Disabling a V2 feature alone does not select V1. V2 native completion can wake an idle owner; V1 requires the native wait tool for required Results. These are synthetic provider decisions, not a measurement of live model autonomy or Ultra.

Work attribution uses an exact direct native call, verified input delivery or prepared native task identity joined with one unique parent activity. V2 canonical child history omits inbound Agent messages. Visible input can prove the exact prompt and receiving turn. Native task-name recovery proves the route through parent metadata, root turn and activity, not encrypted prompt content. Unnamed, missing or ambiguous assignment evidence stays Unknown without duplicate execution. Script text, output text or timing cannot replace native identity. V2 remains direct-only by default; qualification explicitly sets native `multi_agent_v2.non_code_mode_only=false`. Decodex does not change that policy.

Conversation permission changes apply through native thread settings. Defaults apply to newly created conversations, and native children inherit their parent settings. Active turns and already-pending human approvals keep native ownership. Global Guardian now controls optional review records only; it does not override a conversation reviewer. Reviewer selection alone never grants Full Access. Private details retain verified native assistant output and item-free terminal metadata only. They exclude raw reasoning frames and do not enter parent canonical history, complete Work or replay actions. Exact action Review association is unavailable in this runtime and remains explicit. Optional records are limited to 128 at 64 KiB each, with 32 short overview previews; reconnect gaps remain visible. After a completed review, history recovery examines up to 32 same-directory private candidates and five turns each, verifying the parent before retention. The selected runtime cannot subscribe to raw events on resume.

Agent/Material scope precedes SQL page limits. Work/evidence views keep at most 100 records per category and expose additional retained data. Selected evidence uses digest-checked 8 KiB pages up to 32 MiB. Work association does not move the native conversation to another execution directory or create another sandbox. Pause does not roll back existing native actions.

The synthetic cohorts do not establish installed-app, live-account, general model-quality, audible-quality or public release acceptance. The controlled real-provider acceptance above has only its stated scope. Native provenance and local guards do not prove semantic correctness. Keep these limits when reusing qualification after cache or run-directory cleanup.
