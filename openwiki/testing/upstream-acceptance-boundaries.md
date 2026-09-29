---
type: Reference
tags: [decodex, architecture]
title: "Upstream integration acceptance boundaries"
description: "What source audits, native fixtures, signed desktop tests and merges do and do not prove."
sources:
  - id: openwiki-source-c8b1a2a9f2113ec43d4066da
    resource: repo://Makefile.toml
  - id: openwiki-source-76081c1a47ca8cf32593de34
    resource: repo://scripts/macos/test_decodex_app_stage.sh
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T06:24:17.023Z
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

Choose the smallest test that can fail for the changed behavior. Preserve complete failure output and the actual artifact identity. Do not rerun long desktop or physical-provider flows to remove every historical open statement. Use system-temporary fixtures and remove owned test processes and data when done. Documentation consolidation alone does not justify a fresh model call, application launch or complete Rust rebuild.
