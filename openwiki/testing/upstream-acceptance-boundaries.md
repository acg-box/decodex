---
type: Reference
title: "Upstream integration acceptance boundaries"
description: "Source, native fixture, signed desktop, installation and physical voice evidence boundaries."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-0a61b25348131fe1bb6e1d12
    resource: repo://apps/decodex-gpui/src/agent_context_settings_wire_tests.rs
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-02bd3cfc9d4db9d1d1d8fd59
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences_tests.rs
  - id: openwiki-source-8bbd4ba0cf275fa1b4da9acf
    resource: repo://crates/decodex-components/tests/development.rs
  - id: openwiki-source-08dbb31668b58707418687d2
    resource: repo://crates/decodex-components/tests/official_release.rs
  - id: openwiki-source-aee901c2feaee74f17637372
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_compaction_tests.rs
  - id: openwiki-source-4a57fba4bea69171318c6323
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_fork_tests.rs
  - id: openwiki-source-32afc8038b2c3b7df030c573
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_prediction_tests.rs
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
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-283a40a004fda23504fad7e1
    resource: repo://crates/decodex-runtime/src/agent/result_messages.rs
  - id: openwiki-source-5af40fd50adccf5d93ff82a0
    resource: repo://crates/decodex-runtime/src/agent/tests/capacity.rs
  - id: openwiki-source-30c7edf894a639091ecbafc6
    resource: repo://crates/decodex-runtime/src/agent/tests/external_context.rs
  - id: openwiki-source-64d7588c1af50398568a792e
    resource: repo://crates/decodex-runtime/src/agent/timeline.rs
  - id: openwiki-source-74145d84eb83d80ec3062595
    resource: repo://crates/decodex-runtime/src/agent/timeline/metrics.rs
  - id: openwiki-source-ec5c9f32d2135154f4297a49
    resource: repo://crates/decodex-runtime/src/conversation.rs
  - id: openwiki-source-07c9cc033899826079f8efb5
    resource: repo://crates/decodex-runtime/tests/native_qualification/large_catalog.py
  - id: openwiki-source-c772fc057f500f765d09a3c7
    resource: repo://crates/decodex-runtime/tests/native_qualification/log_storage.py
  - id: openwiki-source-88b81a215b1bac061dad80ec
    resource: repo://crates/decodex-runtime/tests/native_qualification/model_errors.py
  - id: openwiki-source-c6bea677adcde563202b65b9
    resource: repo://crates/decodex-runtime/tests/native_qualification/response_identity.py
  - id: openwiki-source-b30d382781a313611ecd221f
    resource: repo://crates/decodex-runtime/tests/native_qualification/thread_settings.py
  - id: openwiki-source-a6440a3d3a3c3121fd4c553f
    resource: repo://database/src/agent_output.rs
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
  - id: openwiki-source-e8c30c6f5849a5077d7b4aa5
    resource: repo://scripts/macos/build_codex_source.test.mts
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-10-24T06:08:33.099Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-24T06:15:55.391Z
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

The [portable qualification record](../../automations/upstream/qualification.json) identifies exact source tests, artifact and helper hashes, reusable probes and limits. The [retained evidence index](../../automations/upstream/retained-evidence.json) identifies the current native consumers. Native run receipts retain raw output; Git retains replaced compact records.

The current source cohort uses a `dev-small` app-server helper with explicit `CARGO_BIN_EXE_*` paths; helper-spawn cases also use the exact signed candidate CLI. The qualification record preserves both roles and the original assertions. Older source fixtures that require private startup arguments retain their own launcher and build limits in Git history. Passing a source fixture does not establish behavior through a production API. Test supported production inputs against the exact selected CLI and helpers as a separate step.

Earlier conditional remote-credential snapshot cases failed because the host resolver returned a reserved address or could not resolve a fixture hostname. Native address protection remained enabled. Current Decodex runtime consumers use app-server stdio and do not provision a remote executor. The record qualifies retained consumers within that scope; it does not qualify the failed conditional remote behavior. Reopen that proof when a concrete retained remote consumer exists or the fixture can use compatible system DNS. Preserve the failure evidence, original assertions and native policy.

## Terminal errors and external context

The terminal projection retains the native turn status and bounds its error details. Capacity retry requires an exact failed-turn readback with `serverOverloaded`. The `quota_other_errors_and_missing_history_do_not_schedule_capacity_retries` test includes the `other` error classification. The `failed_turn_keeps_public_reason_and_explicit_bounds` test checks the public failure reason and its size and privacy limits.

The exact-artifact `model_errors.py` probe covers HTTP 404, SSE `error` and SSE `response.failed` carrying `model_not_found`. Each candidate case makes one request without retry and preserves `modelNotFound` plus the provider message in live, stored and cold history. The native capacity fixture also checks that Decodex does not schedule another attempt. Source tests cover additional HTTP and WebSocket mappings; these source cases are not additional artifact transport executions.

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

The `log_storage.py` probe checks the default diagnostic database, an explicit `log_dir`, and an environment-selected state directory with a separate diagnostic directory. A fourth case verifies that an unavailable diagnostic directory does not prevent unauthenticated `login status`. These cases use system-temporary homes. Managed storage precedence is covered by the recorded source tests; this probe does not qualify NFS or migrate existing databases.

Set `QUALIFIED_CODEX_BINARY`, `QUALIFIED_CODEX_SOURCE_COMMIT` and `DECODEX_QUALIFICATION_OUTPUT` as described in [Commands and validation](../operations/commands-and-validation.md#qualify-retained-native-behavior), then run:

```sh
python3 crates/decodex-runtime/tests/native_qualification/thread_settings.py
python3 crates/decodex-runtime/tests/native_qualification/log_storage.py
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

The native launch fixture checks description-first Code Mode, child context defaults, Agent message wakeups and fork-prefix preservation while retaining boolean and table forms of the user's multi-agent selection. Semantic prefix preservation does not prove provider cache savings. Synthetic context-limit and compaction probes do not prove real-provider latency or cost improvements. Code Mode transport remains controlled by native configuration; this integration adds no transport selector or universal tool-output retention policy.

## Question routing, completion and cancellation

The [retained coordination evidence](../../automations/upstream/retained-evidence.json) links the bounded historical real-model evaluations for self-resolution, peer coordination, parent escalation, user decisions and persistent Agent completion. It retains the current contract for exact inbox receipts, separate child-goal resolution and recovery after an idle-only Busy refusal. Busy means the continuation was not sent or queued. The model should finish its current turn, wait for the existing worker and reassess that same worker before sending a follow-up. The historical evaluations used one model and effort with controlled scenarios and predefined user choices. Current synthetic child-approval and local library regressions are separate requalification; they do not repeat real-model judgment or establish every model, permission configuration, race or installed interaction.

The [sampling qualification](../../automations/upstream/qualification.json) separately records explicit cancellation of Ask separately. Closing its panel only hides presentation; Cancel answer targets the exact native sampling request. A cancellation request, native acknowledgement and a result that wins a race are different outcomes. Preserve exact request ownership and uncertain-state readback. This is not cancellation of the parent conversation or a general scheduler. These receipts do not establish installed-app acceptance.

These fixture definitions specify what can be checked. They do not claim a new execution result from this documentation refresh, installed-app acceptance, or release delivery.
