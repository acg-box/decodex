---
type: Reference
tags: [decodex, architecture]
title: "Model selection and settings ownership"
description: "Native model inheritance, explicit task and turn settings, configuration receipts and known limits."
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T02:19:36.307Z
sources:
  - id: openwiki-source-a2c2a5ced9dfa1aee02eedc2
    resource: repo://crates/decodex-runtime/src/agent_model_settings.rs
  - id: openwiki-source-60a941dcb12f0400ca4cc58f
    resource: repo://docs/archive/upstream-2026-09/model-access-programs.md
  - id: openwiki-source-1c42e27415f1b607c139a462
    resource: repo://docs/archive/upstream-2026-09/native-flex-qualification.md
  - id: openwiki-source-8d4b61fd83ed007c18390abe
    resource: repo://docs/archive/upstream-2026-09/upstream-feature-decisions.md
generated: { by: "codex", at: "2026-09-28T02:19:36.307Z" }
---


# Model selection and settings ownership

## One native settings owner

Codex owns the effective model catalog, task configuration, provider admission and execution settings. Decodex reads and presents these values through a retained connection. Catalog, directory, account, process and task identity belong to the observation. An absent value can mean unknown or inherited; it must not silently become an explicit override.

## Retained choices

| Control | Scope |
| --- | --- |
| Saved task model (O07) | Future turns of the selected task; does not rewrite global defaults |
| Live model/reasoning (O08) | Later steps of the exact active turn when the installed native capability supports it |
| Exact model ID, effort and service tier (O09) | Explicit execution intent, with native validation and inherited settings preserved |
| Ordinary model fallback (O10) | A retained automatic policy for an idle task after current recovery evidence; it does not replay input |
| Permission/reviewer panel (O11) | Native task policy and explicit reviewed settings; not a replacement security engine |
| Task plugins (O12) | The native task exclusion list; not global installation |
| Access-program metadata (O23) | Read-only native catalog information; does not grant access, select Daybreak or change quota |

## Writes and recovery

A review binds the displayed source and current settings to an explicit action. Save once, record the outcome and read back. An acknowledgment can mean queued configuration rather than effective execution state. Another native publication can invalidate a review even if displayed values match.

Shared native configuration writes retain file/version identity and arbitration. Historical model journals remain readable; old unresolved records are not converted into successful modern operations or replayed through a second writer. Queued user execution choices must not be overwritten by an unrelated settings edit.

The user retained these controls. Maintenance can repair their existing behavior; it cannot treat every new upstream setting as permission to add another editor.

## Known native limits

Configured Flex and an explicit runtime change to Flex have different recorded cold-resume outcomes. The archived qualification does not establish the latter across restart. Keep unsupported or unavailable states visible and do not replay settings to hide a mismatch. Native catalog metadata is not evidence of a real enterprise entitlement.

See [plugin and App settings](../integrations/tools-plugins-and-apps.md), [product decisions](../decisions/upstream-product-scope.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md). Representative owners are `agent_model_settings`, native `thread_model_settings`, `live_settings` and the current model journals.
