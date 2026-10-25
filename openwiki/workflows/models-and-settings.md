---
type: Reference
title: "Model selection and settings ownership"
description: "Native model, Goal and search settings, source-bound writes and memory observation."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-ef01be458d459d884afe8624
    resource: repo://apps/decodex-gpui/src/agent_live_settings.rs
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-f2483b817a8847254a871b51
    resource: repo://crates/decodex-codex/src/app_server_client/goals.rs
  - id: openwiki-source-3515e2ff1d1ef96caaa97617
    resource: repo://crates/decodex-codex/src/app_server_client/search_preferences.rs
  - id: openwiki-source-5186d5aa506036d23eae1396
    resource: repo://crates/decodex-codex/src/app_server_client/thread_model_selection.rs
  - id: openwiki-source-7a438b090d3a7042aebaf269
    resource: repo://crates/decodex-codex/src/conversation.rs
  - id: openwiki-source-f5d073da07bcb17ee416f3b5
    resource: repo://crates/decodex-runtime/src/account_launch/process_native_control_tests.rs
  - id: openwiki-source-b2869e6da778a74cb6afc667
    resource: repo://crates/decodex-runtime/src/account_launch/process.rs
  - id: openwiki-source-ba9677fef0b3a23f71d07771
    resource: repo://crates/decodex-runtime/src/agent_capabilities.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-a2c2a5ced9dfa1aee02eedc2
    resource: repo://crates/decodex-runtime/src/agent_model_settings.rs
  - id: openwiki-source-f251e17a7693193da84b2f7a
    resource: repo://crates/decodex-runtime/src/agent_native_goal.rs
  - id: openwiki-source-d28152527a1218d5afb307f7
    resource: repo://crates/decodex-runtime/src/agent_search_settings.rs
  - id: openwiki-source-a4a2658e06fa2cf48ff4589e
    resource: repo://crates/decodex-runtime/src/agent/global_guardian.rs
  - id: openwiki-source-a7b97dd347fbe2f630692d12
    resource: repo://crates/decodex-runtime/src/agent/tests/native_agent_wake.rs
  - id: openwiki-source-07c9cc033899826079f8efb5
    resource: repo://crates/decodex-runtime/tests/native_qualification/large_catalog.py
  - id: openwiki-source-b30d382781a313611ecd221f
    resource: repo://crates/decodex-runtime/tests/native_qualification/thread_settings.py
generated: { by: "codex", at: "2026-10-24T21:11:31.338Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T06:52:57.034Z
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
| Permission profiles | Native execution policy for the selected conversation; Full Access stays separate |
| Global Guardian | Ask Me or Automatic Review for supported sources; no per-Agent reviewer override |
| Review Details | Global optional bounded private reviewer output and lifecycle observations |

## Writes and recovery

A review binds the displayed source and current settings to an explicit action. Save once, record the outcome and read back. An acknowledgment can mean queued configuration rather than effective execution state. Another native publication can invalidate a review even if displayed values match.

Shared native configuration writes retain file/version identity and arbitration. Historical model journals remain readable; old unresolved records are not converted into successful modern operations or replayed through a second writer. Queued user execution choices must not be overwritten by an unrelated settings edit.

Cold resume inherits the saved native thread settings without reapplying creation settings. A model-only update preserves the native tier, provider and collaboration settings. The reusable qualification probe checks that the saved service tier and summary setting survive changed global defaults in both legacy and paginated thread modes. This evidence applies to the selected qualified artifact; it does not establish installed-app acceptance. See [saved settings and diagnostic storage](../testing/upstream-acceptance-boundaries.md#saved-settings-and-diagnostic-storage) for the commands and limits.

Plugin installation and selection are configured in Codex. Maintenance of the current model controls does not authorize a new editor for every upstream setting.

## Guardian and helper settings

Global Guardian stores desired reviewer intent and reports actual native per-source readback. Its thread preference affects subsequent turns. Active-turn policy, already-pending human approvals and previously spawned child captures retain native ownership. The old per-Agent reviewer control is read-only evidence, and old per-Agent write commands are rejected. Live model/reasoning controls remain source-bound supported native edits.

Full Access is a native permission choice, not a Guardian mode. Turning optional Review Details off stops new observation; it does not approve pending actions or remove native enforcement. See [approvals](approvals-and-native-ownership.md).

Native model metadata and configuration select V1 or V2 helper behavior. The selected artifact can choose V2 even when its feature flag is false. The synthetic native matrix uses V2 `gpt-5.6-sol` and V1 `gpt-5.6-luna`, both at Medium; neither requires Ultra. V2 idle-owner continuation and V1 native wait remain native execution. A selected effort level alone is not proof of better orchestration or model judgment.

## Native Goal editing

The Goal panel reads and edits the Goal attached to the exact owned native thread. A new Goal requires an objective and an explicit start or pause choice. An edit can keep, set or reset its token budget and can explicitly change status. Native policy enforces the allowed budget and status transition. Decodex does not create a second persistent Goal engine.

The reviewed source, thread and Goal identity must still match before a write. Long objectives use a native objective attachment instead of silently truncating the submitted objective. Display truncation is marked separately. An uncertain save requires a fresh native read; it is not permission to repeat the mutation.

## Search defaults and memory observation

The search control shows the saved user preference separately from the effective project default. Supported native modes include disabled, cached, indexed and live, subject to current native requirements. Saving writes only `web_search` against the reviewed native file version, with no user-config reload. It does not restart, resume or fork an existing conversation to force the new default into its loaded settings.

Memory availability comes from the native feature observation. A capability flag is not a separate Decodex memory database or a retention policy. Periodic distillation and automatic thread cleanup require their own product decision.

## Native launch defaults

The attested app-server launch enables description-first Code Mode, native child context limits and Agent message wakeups. It also enables fork-prefix preservation through the dotted `features.multi_agent_v2.preserve_fork_prefix` option, which preserves the user's existing multi-agent enablement. These are runtime launch defaults, not new desktop switches. Description ordering does not itself enable Code Mode.

Native Codex retains transport selection. The integration does not force shared gRPC hosting or a global tool-output retention setting. Fork-prefix preservation and child context limits have bounded compatibility evidence; they do not establish provider cache savings, latency improvements or better model answers. Follow [acceptance boundaries](../testing/upstream-acceptance-boundaries.md) when evaluating those effects.

## Explicit model catalogs

Native model discovery accepts an explicitly configured catalog larger than 1 MiB in the current qualified runtime. The reusable `large_catalog.py` fixture checks that its selected model reaches `model/list`. This is a compatibility contract for native configuration; it does not add a provider or account setup control. The existing account-plan boundary remains unchanged.

## Known native limits

Verify explicit service-tier changes across cold resume with the installed binary. A configured default and a runtime override are different paths. Keep unsupported or unavailable states visible and do not replay settings to hide a mismatch. Native catalog metadata is not evidence of a real enterprise entitlement.

See [plugin and App settings](../integrations/tools-plugins-and-apps.md), [product decisions](../decisions/upstream-product-scope.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md). Representative owners are `agent_model_settings`, native `thread_model_settings`, `live_settings` and the current model journals.
