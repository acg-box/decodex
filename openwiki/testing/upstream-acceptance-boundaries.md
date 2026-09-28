---
type: Reference
tags: [decodex, architecture]
title: "Upstream integration acceptance boundaries"
description: "What source audits, native fixtures, signed desktop tests and merges do and do not prove."
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T02:19:36.307Z
sources:
  - id: openwiki-source-4753c3bdf2916fd807c75377
    resource: repo://docs/archive/upstream-2026-09/native-child-mcp-qualification.md
  - id: openwiki-source-1c42e27415f1b607c139a462
    resource: repo://docs/archive/upstream-2026-09/native-flex-qualification.md
  - id: openwiki-source-c29a139ba78abf924bfd2cc2
    resource: repo://docs/archive/upstream-2026-09/signed-draft-acceptance.md
  - id: openwiki-source-8d4b61fd83ed007c18390abe
    resource: repo://docs/archive/upstream-2026-09/upstream-feature-decisions.md
generated: { by: "codex", at: "2026-09-28T02:19:36.307Z" }
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

The fixed upstream scan covered 1,569 commits. The inherited audit classified 360 paths. The user review grouped eight core areas and 26 optional rows, including restored baseline behavior. These numbers are not interchangeable.

## Recorded desktop evidence

The [signed draft acceptance](../../docs/archive/upstream-2026-09/signed-draft-acceptance.md) names artifact `cc0895c3a440a544d6c414028456abd0b469cc77`. It records ordinary Agent composer input retained over restart without submission, shared-store conflict, cancelled Quit and export, plus response/recap display on return. Earlier records for `0658` or `a15fe830` apply to their own artifacts.

The evidence does not establish a general focus fix for every attached window. It does not turn a cancelled long background timer test into a pass. Physical voice, Dock behavior and broader lifecycle coverage must retain their stated limits. Removed private fixture directories are historical locations, not current downloadable evidence.

## Native and product limits

- Explicit runtime Flex changes are not qualified across cold resume in the same way as configured Flex.
- Native child MCP browser-auth/user-input has a recorded root-handoff mismatch on the tested binary.
- Physical voice/WebRTC and late remote caption identity retain their precise recorded limits.
- Independent browser/device-code enrollment does not establish a global native enrollment-policy owner.
- MCP App streams, widgets without captured resource URIs and general browser capabilities are not delivered by the current widget host.
- Full Analytics dashboards, a Daybreak selector, generic experimental settings and an external memory service are not local feature deliveries merely because upstream APIs or research exist.

See [native limits](../../docs/archive/upstream-2026-09/source-preservation-native-limits.md), [child MCP qualification](../../docs/archive/upstream-2026-09/native-child-mcp-qualification.md), [voice records](../../docs/archive/upstream-2026-09/voice-settings.md), and [product decisions](../decisions/upstream-product-scope.md).

## Proportionate validation

Choose the smallest test that can fail for the changed behavior. Preserve complete failure output and the actual artifact identity. Do not rerun long desktop or physical-provider flows to remove every historical open statement. Use system-temporary fixtures and remove owned test processes and data when done. Documentation consolidation alone does not justify a fresh model call, application launch or complete Rust rebuild.
