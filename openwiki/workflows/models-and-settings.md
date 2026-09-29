---
type: Reference
title: "Model selection and settings ownership"
description: "Native model, Goal and search settings, source-bound writes and memory observation."
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T13:52:19.644Z
sources:
  - id: openwiki-source-f2483b817a8847254a871b51
    resource: repo://crates/decodex-codex/src/app_server_client/goals.rs
  - id: openwiki-source-3515e2ff1d1ef96caaa97617
    resource: repo://crates/decodex-codex/src/app_server_client/search_preferences.rs
  - id: openwiki-source-ba9677fef0b3a23f71d07771
    resource: repo://crates/decodex-runtime/src/agent_capabilities.rs
  - id: openwiki-source-a2c2a5ced9dfa1aee02eedc2
    resource: repo://crates/decodex-runtime/src/agent_model_settings.rs
  - id: openwiki-source-f251e17a7693193da84b2f7a
    resource: repo://crates/decodex-runtime/src/agent_native_goal.rs
  - id: openwiki-source-d28152527a1218d5afb307f7
    resource: repo://crates/decodex-runtime/src/agent_search_settings.rs
generated: { by: "codex", at: "2026-09-29T13:52:19.644Z" }
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

## Native Goal editing

The Goal panel reads and edits the Goal attached to the exact owned native thread. A new Goal requires an objective and an explicit start or pause choice. An edit can keep, set or reset its token budget and can explicitly change status. Native policy enforces the allowed budget and status transition. Decodex does not create a second persistent Goal engine.

The reviewed source, thread and Goal identity must still match before a write. Long objectives use a native objective attachment instead of silently truncating the submitted objective. Display truncation is marked separately. An uncertain save requires a fresh native read; it is not permission to repeat the mutation.

## Search defaults and memory observation

The search control shows the saved user preference separately from the effective project default. Supported native modes include disabled, cached, indexed and live, subject to current native requirements. Saving writes only `web_search` against the reviewed native file version, with no user-config reload. It does not restart, resume or fork an existing conversation to force the new default into its loaded settings.

Memory availability comes from the native feature observation. A capability flag is not a separate Decodex memory database or a retention policy. Periodic distillation and automatic thread cleanup require their own product decision.

## Known native limits

Verify explicit service-tier changes across cold resume with the installed binary. A configured default and a runtime override are different paths. Keep unsupported or unavailable states visible and do not replay settings to hide a mismatch. Native catalog metadata is not evidence of a real enterprise entitlement.

See [plugin and App settings](../integrations/tools-plugins-and-apps.md), [product decisions](../decisions/upstream-product-scope.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md). Representative owners are `agent_model_settings`, native `thread_model_settings`, `live_settings` and the current model journals.
