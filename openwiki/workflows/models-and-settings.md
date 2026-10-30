---
type: Reference
title: "Model selection and settings ownership"
description: "Native model, Goal and search settings, source-bound writes and memory observation."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-4e2d2760a1fe5d904918afb7
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ConversationEstimatesView.swift
  - id: openwiki-source-b243cfb8ae3053f8047fdab0
    resource: repo://apps/decodex-gpui/src/agent_capabilities.rs
  - id: openwiki-source-c76550e85b25876b339f9255
    resource: repo://apps/decodex-gpui/src/agent_context_settings.rs
  - id: openwiki-source-9c68c30b2ff24ddf9a764d74
    resource: repo://apps/decodex-gpui/src/agent_memory.rs
  - id: openwiki-source-c6ac7d6b2fdeb458faab2c3a
    resource: repo://apps/decodex-gpui/src/agent_permissions.rs
  - id: openwiki-source-0f9840ddf9e841a062aa905a
    resource: repo://apps/decodex-gpui/src/agent_prompt_suggestions.rs
  - id: openwiki-source-296a92a9cefecb19f0fc1f95
    resource: repo://apps/decodex-gpui/src/agent_sample_tests.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-40c9c6a55485557343853517
    resource: repo://apps/decodex-gpui/src/agent_usage_estimates.rs
  - id: openwiki-source-0d053d4de2f668168f982629
    resource: repo://apps/decodex-gpui/src/agent_usage_evidence.rs
  - id: openwiki-source-682f90c6d08ed3207257f1d5
    resource: repo://apps/decodex-gpui/src/conversations.rs
  - id: openwiki-source-a2f254020dd8eaabd6366c91
    resource: repo://apps/decodex-gpui/src/session_settings.rs
  - id: openwiki-source-37be5bb3dc64279420e57b31
    resource: repo://apps/decodex-gpui/src/settings_native.rs
  - id: openwiki-source-a16804a567889414868e5d67
    resource: repo://apps/decodex-gpui/src/settings_surface.rs
  - id: openwiki-source-546812ff3637690f4a184b89
    resource: repo://apps/decodex-gpui/src/ui_preferences.rs
  - id: openwiki-source-f580b10e16f1d25ebfea3bf8
    resource: repo://automations/upstream/qualification.json
  - id: openwiki-source-3127fd4c7b2aefcc9cea7961
    resource: repo://automations/upstream/retained-contracts.json
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-451b1382eb8cdcb9915ac78b
    resource: repo://crates/decodex-app-client-ffi/src/conversation_estimates.rs
  - id: openwiki-source-2b08587e9b26e203156110c8
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences.rs
  - id: openwiki-source-f2483b817a8847254a871b51
    resource: repo://crates/decodex-codex/src/app_server_client/goals.rs
  - id: openwiki-source-9819ced71ede01a1ddc5af56
    resource: repo://crates/decodex-codex/src/app_server_client/memory.rs
  - id: openwiki-source-3515e2ff1d1ef96caaa97617
    resource: repo://crates/decodex-codex/src/app_server_client/search_preferences.rs
  - id: openwiki-source-787f8ad27b8519fbed2bd039
    resource: repo://crates/decodex-codex/src/app_server_client/thread_fork.rs
  - id: openwiki-source-5186d5aa506036d23eae1396
    resource: repo://crates/decodex-codex/src/app_server_client/thread_model_selection.rs
  - id: openwiki-source-7a438b090d3a7042aebaf269
    resource: repo://crates/decodex-codex/src/conversation.rs
  - id: openwiki-source-0d29990b5fd295fa89a495ad
    resource: repo://crates/decodex-protocol/src/model_catalog.rs
  - id: openwiki-source-4a57fba4bea69171318c6323
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_fork_tests.rs
  - id: openwiki-source-e5be8be8499e6ccee0cf7ad0
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_memory_tests.rs
  - id: openwiki-source-f5d073da07bcb17ee416f3b5
    resource: repo://crates/decodex-runtime/src/account_launch/process_native_control_tests.rs
  - id: openwiki-source-b2869e6da778a74cb6afc667
    resource: repo://crates/decodex-runtime/src/account_launch/process.rs
  - id: openwiki-source-ba9677fef0b3a23f71d07771
    resource: repo://crates/decodex-runtime/src/agent_capabilities.rs
  - id: openwiki-source-1477615a268800361379ce21
    resource: repo://crates/decodex-runtime/src/agent_initial_settings.rs
  - id: openwiki-source-dc51b38db429ec5fe466c5f2
    resource: repo://crates/decodex-runtime/src/agent_memory.rs
  - id: openwiki-source-81ce54e50da763fa2a1f8813
    resource: repo://crates/decodex-runtime/src/agent_model_recovery.rs
  - id: openwiki-source-a2c2a5ced9dfa1aee02eedc2
    resource: repo://crates/decodex-runtime/src/agent_model_settings.rs
  - id: openwiki-source-f251e17a7693193da84b2f7a
    resource: repo://crates/decodex-runtime/src/agent_native_goal.rs
  - id: openwiki-source-5bc08ad67fcdb104dfcde7bf
    resource: repo://crates/decodex-runtime/src/agent_permissions.rs
  - id: openwiki-source-d28152527a1218d5afb307f7
    resource: repo://crates/decodex-runtime/src/agent_search_settings.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-a7b97dd347fbe2f630692d12
    resource: repo://crates/decodex-runtime/src/agent/tests/native_agent_wake.rs
  - id: openwiki-source-07c9cc033899826079f8efb5
    resource: repo://crates/decodex-runtime/tests/native_qualification/large_catalog.py
  - id: openwiki-source-b30d382781a313611ecd221f
    resource: repo://crates/decodex-runtime/tests/native_qualification/thread_settings.py
  - id: openwiki-source-8c78df4b382026886ac50894
    resource: repo://database/src/usage_evidence.rs
generated: { by: "codex", at: "2026-10-30T20:00:55.995Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-30T20:00:55.995Z
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
| Permissions | Composer shield: Ask for approval, Approve for me, Full access or Read-only for the selected conversation |
| New-conversation defaults | Settings > Agents; apply only when a new conversation starts |
| Speed | Native advertised speed choices in the model menu |
| Review Details | Global optional bounded private reviewer output and lifecycle observations |

## Find the retained switches

| Control | Location |
| --- | --- |
| Background compaction | Settings > Agents > Native defaults |
| Keep delegation instructions after compaction | Settings > Agents > Native defaults |
| Cached MCP catalog, one switch per configured server | Settings > Agents > Native defaults |
| Automatic next-message suggestions | Settings > Agents > Conversation assistance |
| Automatic task recaps | Settings > General |
| Permissions and speed for new Agents | Settings > Agents |
| Model, reasoning, search, voice and Goal for the selected Agent | Composer and Agent preferences |
| Optional EGM component | Settings > Components |

Open Settings > Agents after selecting a connected Agent. The native section reads through that Agent's account and project. It uses the same controls and native configuration as Agent preferences > Context defaults. Saved user values and effective project values remain separate; project or managed policy can override a saved choice. Saving does not restart loaded conversations. Decodex keeps no second copy of these native preferences.

The cached-catalog switch writes only `startup_readiness` for an existing user-configured MCP server: on selects `catalog`, and off selects `connection`. Native Codex owns the cache and tool connection. A missing or invalid cache keeps normal discovery; a tool call still needs the live connection. Configure project-only and plugin servers in Codex.

Automatic suggestions are enabled by default. Turning the switch off prevents future automatic requests. Turning it back on does not request suggestions for answers that completed while it was off. Tab acceptance still creates an undoable draft and never sends it. This setting does not turn on content export or the deferred messaging experiments.

The [retained settings evidence](../../automations/upstream/retained-evidence.json) separates the 23 targeted checks, 12 incidental unit checks and exact-artifact configuration roundtrip from installed-app acceptance. The remaining manual proposals are outside this settings audit.

## Writes and recovery

A review binds the displayed source and current settings to an explicit action. Save once, record the outcome and read back. An acknowledgment can mean queued configuration rather than effective execution state. Another native publication can invalidate a review even if displayed values match.

Shared native configuration writes retain file/version identity and arbitration. Historical model journals remain readable; old unresolved records are not converted into successful modern operations or replayed through a second writer. Queued user execution choices must not be overwritten by an unrelated settings edit.

Cold resume inherits the saved native thread settings without reapplying creation settings. A model-only update preserves the native tier, provider and collaboration settings. The reusable qualification probe checks that the saved service tier and summary setting survive changed global defaults in both legacy and paginated thread modes. The probe also checks acknowledged approval, reviewer and read-only permission recovery despite stale turn context during fork and cold resume in both history modes. Each recovery case makes one inference request and does not replay the saved turn. This evidence applies to the selected qualified artifact; it does not establish installed-app acceptance. See [saved settings and diagnostic storage](../testing/upstream-acceptance-boundaries.md#saved-settings-and-diagnostic-storage) for the commands and limits.

Plugin installation and selection are configured in Codex. Maintenance of the current model controls does not authorize a new editor for every upstream setting.

## Native model retirement

The desktop reads `model/list` from the current native connection and refreshes its Agent catalog at a 60-second interval. Account, directory and process changes invalidate the observation. Model and effort cycling require that catalog; an unavailable read does not provide a fixed fallback list.

When the same complete catalog gives the selected model a retirement deadline that has passed, Decodex uses the catalog's explicit current default. This applies to new Agent settings, new Agents in an existing shared process, and loaded idle Agent recovery. Shared process configuration stays unchanged. An upgrade suggestion alone, a missing model, a network failure or an account quota response does not prove retirement. A usable selected model stays selected.

Recovery uses the existing source-bound settings journal and publication readback. It preserves compatible effort and service-tier choices, does not change provider or permission ownership, and does not replay input. Active turns keep their current execution. A native session that cannot be loaded remains outside this retirement check.

The [retained evidence](../../automations/upstream/retained-evidence.json) records 24 runtime and 18 desktop tests, with a failing-before and passing-after retirement case. The fixtures cover native-default selection, future or absent deadlines, missing defaults, source changes, draft preservation and no replay. They do not qualify a new native artifact, a real retired provider model or an installed application.

## Guardian and helper settings

The composer shield and Settings > Agents use the same permission presets. Settings defines creation defaults; the shield reads and changes the selected conversation through native permission selection. Native children inherit their parent settings. Independent conversations retain their own choices. Active turns and pending human approvals keep native ownership. Global Guardian only controls optional Review Details. Live model/reasoning controls remain source-bound native edits. Speed stays in the model menu.

Full Access is a native permission choice, not a Guardian mode. Turning optional Review Details off stops new observation; it does not approve pending actions or remove native enforcement. See [approvals](approvals-and-native-ownership.md).

Native model metadata and configuration select V1 or V2 helper behavior. The selected artifact can choose V2 even when its feature flag is false. The synthetic native matrix uses V2 `gpt-5.6-sol` and V1 `gpt-5.6-luna`, both at Medium; neither requires Ultra. V2 idle-owner continuation and V1 native wait remain native execution. A selected effort level alone is not proof of better orchestration or model judgment.

## Native Goal editing

The Goal panel reads and edits the Goal attached to the exact owned native thread. A new Goal requires an objective and an explicit start or pause choice. An edit can keep, set or reset its token budget and can explicitly change status. Native policy enforces the allowed budget and status transition. Decodex does not create a second persistent Goal engine.

The reviewed source, thread and Goal identity must still match before a write. Long objectives use a native objective attachment instead of silently truncating the submitted objective. Display truncation is marked separately. An uncertain save requires a fresh native read; it is not permission to repeat the mutation.

**Attached branch after this turn** keeps the source provider even when the configured default changes. It starts without a copied native Goal and leaves the source Goal unchanged. Creating or opening the branch sends no input. Continue it with a separate explicit message, or use its normal Goal controls after selecting that branch. The selected runtime qualification checks ordinary forks in both history modes, attached creation in paginated history and explicit native rejection of legacy attachment. It checks restart separately and does not certify the installed desktop. A newer candidate that rejects cold resume after a provider-default change remains unselected; source review alone does not replace this retained contract. See [conversation branches](conversations-and-recovery.md#edit-or-branch-at-an-earlier-input).

## Search defaults and memory observation

The search control shows the saved user preference separately from the effective project default. Supported native modes include disabled, cached, indexed and live, subject to current native requirements. Saving writes only `web_search` against the reviewed native file version, with no user-config reload. It does not restart, resume or fork an existing conversation to force the new default into its loaded settings.

Memory availability comes from the native feature observation. A capability flag is not a separate Decodex memory database or a retention policy. Periodic distillation and automatic thread cleanup require their own product decision.

## Read a native memory summary

Open **Agent preferences > Memory summary** for the selected Agent conversation. The first open and **Refresh summary** explicitly read native `thread/memory/read`. Native Codex selects its configured memory version; Decodex does not choose a fallback file or merge versions. Disabled, missing, null or blank memory produces an empty state. An unsupported method, unloaded context or changed source remains unavailable. Open the conversation normally before trying again; inspection does not resume it implicitly.

The view preserves the returned summary text. **Copy summary** copies it only after a click, without changing the composer draft or treating summary text as instructions. Closing the panel clears the view. A replaced Agent/thread/runtime binding discards the old summary. The service bounds the read to 1 MiB and transfers digest-bound 8 KiB UTF-8 pages tied to the exact account, process, native connection and history revision.

Only the native read method is admitted through the retained bridge. This view does not write memory, generate a summary, change feature configuration or add distillation, retention or cleanup policy. The exact-artifact fixture checks native versions and empty states, including an unstarted thread, with zero model requests. The UI regression checks original copy text, draft preservation and source invalidation. See [Selected read-only views](../testing/upstream-acceptance-boundaries.md#selected-read-only-views) for the exact artifact and unmeasured installed-app limits.

**Actual usage** in Agent preferences and Work details reads retained native turn receipts. It reports known token deltas and native duration with source coverage. Work totals include only exactly linked, unshared turns; missing child/history evidence remains unknown. These totals do not establish billing, complete Work cost or elapsed time. The view adds no execution or automatic allocation policy.

## Conversation cost estimates

Use **Cost estimate** in the conversation header to read the existing native provider estimate. The compact amount shows approximate USD when reported, otherwise credits. Open the details for the source account, observation time and **Refresh estimate**. Closing and reopening cached details does not issue another read. A changed conversation or runtime source discards the old observation.

The Menu Bar account surface has **Conversation estimates** and **Refresh estimates**. Opening it reads up to eight recent locally managed native conversations with available sources. Each reported row retains its source-account label. Email visibility follows the existing privacy preference. The view is a recent-conversation list, not an account total, and unavailable conversations stay unavailable.

Native estimates can lag activity. A missing USD value is not zero; reported zero stays zero. No local model price table, incomplete account sum, final-bill promise or quota calculation is added. The main view and Menu Bar reuse the existing service usage path. See [the retained qualification](../../automations/upstream/retained-evidence.json) for the `history_resource_cost` entry, named checks and installed-app limits.

## Native launch defaults

The attested app-server launch requests native child context limits and Agent message wakeups. Its retained description-first argument is accepted as a removed native feature; current ordering is native-owned. It also enables fork-prefix preservation through the dotted `features.multi_agent_v2.preserve_fork_prefix` option, which preserves the user's existing multi-agent enablement. These are runtime launch defaults, not new desktop switches. Description ordering does not itself enable Code Mode.

Native Codex retains transport selection. The integration does not force shared gRPC hosting or a global tool-output retention setting. Fork-prefix preservation and child context limits have bounded compatibility evidence; they do not establish provider cache savings, latency improvements or better model answers. Follow [acceptance boundaries](../testing/upstream-acceptance-boundaries.md) when evaluating those effects.

## Explicit model catalogs

Native model discovery accepts an explicitly configured catalog larger than 1 MiB in the current qualified runtime. The reusable `large_catalog.py` fixture checks that its selected model reaches `model/list`. This is a compatibility contract for native configuration; it does not add a provider or account setup control. The existing account-plan boundary remains unchanged.

## Known native limits

Verify explicit service-tier changes across cold resume with the installed binary. A configured default and a runtime override are different paths. Keep unsupported or unavailable states visible and do not replay settings to hide a mismatch. Native catalog metadata is not evidence of a real enterprise entitlement.

See [plugin and App settings](../integrations/tools-plugins-and-apps.md), [product decisions](../decisions/upstream-product-scope.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md). Representative owners are `agent_model_settings`, native `thread_model_settings`, `live_settings` and the current model journals.
