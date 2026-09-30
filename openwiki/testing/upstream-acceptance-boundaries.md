---
type: Reference
title: "Upstream integration acceptance boundaries"
description: "Source, native fixture, signed desktop, installation and physical voice evidence boundaries."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-4a57fba4bea69171318c6323
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_fork_tests.rs
  - id: openwiki-source-68238f7343c9fc1bd11783f3
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_recap_socket_tests.rs
  - id: openwiki-source-ec5c9f32d2135154f4297a49
    resource: repo://crates/decodex-runtime/src/conversation.rs
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-09-29T13:52:19.644Z" }
verified:
  - by: openwiki/0.6.1
    at: 2026-09-30T09:30:20.448Z
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

Place an execution fixture under the operating-system user's home, outside Codex-owned roots. Overriding `HOME` does not change the operating-system account home used by the selected-working-directory check. A fixture under `/tmp` can therefore fail before creating a conversation. Keep local socket paths short enough for the platform. Do not loosen product path checks to make a test run.

PAT tests separately cover versioned import, native identity hydration, optional OAuth-only fields, exact child environment binding and shared-auth Route. Synthetic credentials do not prove the entitlement or validity of a production PAT. Never use production account data to repair a fixture setup failure.
