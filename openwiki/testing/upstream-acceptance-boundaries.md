---
type: Reference
title: "Upstream integration acceptance boundaries"
description: "Source, native fixture, signed desktop, installation and physical voice evidence boundaries."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-4a57fba4bea69171318c6323
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_fork_tests.rs
  - id: openwiki-source-32afc8038b2c3b7df030c573
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_prediction_tests.rs
  - id: openwiki-source-21c0991a686dae637a3616cd
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_read_state_tests.rs
  - id: openwiki-source-68238f7343c9fc1bd11783f3
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_recap_socket_tests.rs
  - id: openwiki-source-f5d073da07bcb17ee416f3b5
    resource: repo://crates/decodex-runtime/src/account_launch/process_native_control_tests.rs
  - id: openwiki-source-ec5c9f32d2135154f4297a49
    resource: repo://crates/decodex-runtime/src/conversation.rs
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-10-10T06:49:11.265Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-10T06:49:11.265Z
---

# Upstream integration acceptance boundaries

## Separate evidence levels

| Evidence | What it establishes | What it does not establish |
| --- | --- | --- |
| Source/consumer reconciliation | A capability has a current owner or an explicit retirement | Successful execution |
| Focused unit or socket fixture | The checked local contract and failure case | An external provider or real desktop interaction |
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

## Native branch and credential fixtures

The ignored local-socket fixture can select native branch qualification with `DECODEX_TEST_NATIVE_FORK`. It requires an explicitly selected native binary, matching isolated `HOME` and `DECODEX_TEST_ACCOUNT_HOME`, and its fixture marker. The provider and accounts are synthetic. It checks an empty branch before the first input, an inclusive branch after that turn, preserved source history, distinct native identities, repeated-command readback and canonical draft handback. It asserts that branch creation and recovery do not add model requests.

For the ordinary Conversation selected-directory path, place the selected workspace under the operating-system user's home, outside Codex-owned roots. Use an existing task-owned workspace for this boundary and keep disposable account homes and other fixture data in system-temporary directories. Overriding `HOME` does not change the operating-system account home used by the selected-working-directory check. A fixture under `/tmp` can therefore fail before creating a conversation. Direct native read-state and prediction fixtures use system-temporary directories and do not invoke that ordinary Conversation directory selector. Keep local socket paths short enough for the platform. Do not loosen product path checks to make a test run.

PAT tests separately cover versioned import, native identity hydration, optional OAuth-only fields, exact child environment binding and shared-auth Route. Synthetic credentials do not prove the entitlement or validity of a production PAT. Never use production account data to repair a fixture setup failure.

## Native additions and limits

The ignored read-state fixture checks explicit read/unread marks, stale-revision refusal, persistence after a cold reopen and unavailable receipts for ephemeral threads. Reading metadata does not acknowledge it or make a model request.

The ignored prediction fixture checks parent context and tool-catalog inheritance, cancellation before inference, cleanup, absence from durable history and unchanged parent metadata. Prediction inherits tools and permissions; it is not the isolated recap path. This capability does not add automatic suggestions or a new product entrypoint.

The native launch fixture checks description-first Code Mode, child context defaults and fork-prefix preservation while retaining boolean and table forms of the user's multi-agent selection. Semantic prefix preservation does not prove provider cache savings. Synthetic context-limit and compaction probes do not prove real-provider latency or cost improvements. Code Mode transport remains controlled by native configuration; this integration adds no transport selector or universal tool-output retention policy.

These fixture definitions specify what can be checked. They do not claim a new execution result from this documentation refresh, installed-app acceptance, or release delivery.
