---
type: Reference
tags: [decodex, architecture]
title: "Model selection and settings ownership"
description: "Native model inheritance, explicit task and turn settings, configuration receipts and known limits."
sources:
  - id: openwiki-source-a2c2a5ced9dfa1aee02eedc2
    resource: repo://crates/decodex-runtime/src/agent_model_settings.rs
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T06:24:17.023Z
---


# Model selection and settings ownership

## One native settings owner

Codex owns the effective model catalog, task configuration, provider admission and execution settings. Decodex reads and presents these values through a retained connection. Catalog, directory, account, process and task identity belong to the observation. An absent value can mean unknown or inherited; it must not silently become an explicit override.

## Retained choices

| Control | Scope |
| --- | --- |
| Saved task model | Future turns of the selected task; does not rewrite global defaults |
| Live model/reasoning | Later steps of the exact active turn when the installed native capability supports it |
| Exact model ID, effort and service tier | Explicit execution intent, with native validation and inherited settings preserved |
| Ordinary model fallback | A retained automatic policy for an idle task after current recovery evidence; it does not replay input |
| Permission/reviewer panel | Native task policy and explicit reviewed settings; not a replacement security engine |

## Writes and recovery

A review binds the displayed source and current settings to an explicit action. Save once, record the outcome and read back. An acknowledgment can mean queued configuration rather than effective execution state. Another native publication can invalidate a review even if displayed values match.

Shared native configuration writes retain file/version identity and arbitration. Historical model journals remain readable; old unresolved records are not converted into successful modern operations or replayed through a second writer. Queued user execution choices must not be overwritten by an unrelated settings edit.

Plugin installation and selection are configured in Codex. Maintenance of the current model controls does not authorize a new editor for every upstream setting.

## Known native limits

Verify explicit service-tier changes across cold resume with the installed binary. A configured default and a runtime override are different paths. Keep unsupported or unavailable states visible and do not replay settings to hide a mismatch. Native catalog metadata is not evidence of a real enterprise entitlement.

See [plugin and App settings](../integrations/tools-plugins-and-apps.md), [product decisions](../decisions/upstream-product-scope.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md). Representative owners are `agent_model_settings`, native `thread_model_settings`, `live_settings` and the current model journals.
