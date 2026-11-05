---
type: Reference
title: "Native tools and connection ownership"
description: "Codex-owned plugin setup, enabled skill selection, host skill roots and retired HTML execution."
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-08ce10c44b7d18a074304e2e
    resource: repo://apps/decodex-gpui/src/agent_hooks.rs
  - id: openwiki-source-01379a7fb49ab2d638863891
    resource: repo://apps/decodex-gpui/src/agent_skills.rs
  - id: openwiki-source-f339a4d1910db63ca305c2df
    resource: repo://apps/decodex-gpui/src/assistant_settings.rs
  - id: openwiki-source-59ed67d26d1c310707a365e5
    resource: repo://apps/decodex-gpui/src/component_plugins.rs
  - id: openwiki-source-0813b2e35e2133631c0aa4db
    resource: repo://apps/decodex-gpui/src/settings_components.rs
  - id: openwiki-source-e379b8a714d71f87371ae328
    resource: repo://crates/decodex-components/src/plugins.rs
  - id: openwiki-source-69e9fd7efd8c3cd616184e27
    resource: repo://crates/decodex-components/src/ragflow_service.rs
  - id: openwiki-source-9ecfe8c96199e765ca2b4e1b
    resource: repo://crates/decodex-components/src/ragflow.rs
  - id: openwiki-source-eccf08613126ff810078b520
    resource: repo://crates/decodex-protocol/src/agent.rs
  - id: openwiki-source-a0063c7b07a1bc990ee9af6c
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-e9d609e612bb7e44111ec4b1
    resource: repo://crates/decodex-runtime/src/agent_integrations.rs
  - id: openwiki-source-5c092afccd8c16040c6d9db2
    resource: repo://crates/decodex-runtime/src/agent_skill_roots.rs
  - id: openwiki-source-8b32cad13ab2428dd54bd986
    resource: repo://crates/decodex-runtime/src/agent_skills.rs
  - id: openwiki-source-13f64682e8d77fd2c031de39
    resource: repo://crates/decodex-runtime/src/agent/assistant.rs
  - id: openwiki-source-1bebc823fa8b6426acb4d5e2
    resource: repo://crates/decodex-runtime/src/agent/native_settings.rs
  - id: openwiki-source-3edc2f094826ee56ddec6b73
    resource: repo://crates/decodex-runtime/src/ragflow.rs
generated: { by: "codex", at: "2026-11-04T14:22:18.349Z" }
verified:
  - by: openwiki/0.7.3
    at: 2026-11-05T05:20:04.159Z
---

# Native tools and connection ownership

## Native discovery and execution

The service reads MCP status, installed plugins and Apps independently. It binds results to the selected native thread, directory and settings guard. Unavailable, unsupported, empty and over-capacity results have different meanings. Ordinary inventory reads do not force a refresh or authorize installation.

Codex owns native tool discovery, filtering, plugin activation and execution. Decodex must not create a second MCP connection or a local tool-policy engine to imitate native behavior. Exact server and plugin names are identities, not display aliases.

## Plugin configuration belongs to Codex

The desktop shows tool status and source ownership. Configure general plugins and connections in Codex for the selected account. Generic local task-plugin selection, sync and authentication-management controls are retired. Native settings observations remain available. The retired local selection journals, query types and mutation commands are removed.

The explicit [EGM Component](elephant-goldfish.md) is a narrow native-lifecycle adapter in Settings > Components. It installs, updates and removes the original plugin through the Codex CLI. It does not restore the retired Agent plugin-management API or add a second marketplace cache.

## Native settings boundaries

The built-in [Assistant lifecycle](../decisions/assistant-lifecycle.md) exposes RAGFlow as an optional managed service component. Install prepares RAGFlow nightly, compatible Infinity and its private service account. Expanding installed RAGFlow directly opens the model form for an OpenAI-compatible Base URL, optional API key and custom extraction and embedding model names. Save requests native verification before saving and automatic dedicated Memory setup. The component installs no model runtime or weights. It requires a running Docker engine. The row shares version metadata, lifecycle buttons, disclosure and inline feedback with the other components. Update reads the actual installed version; Uninstall stops the owned project and keeps its data for reinstall. Service connection is automatic, with no Connect action or service-account form.

The HTTP adapter uses explicit dataset, Memory, Assistant and user scopes. It saves an origin-bound private component credential; settings retain only its reference. Legacy service-environment references remain supported. Knowledge retrieval and Memory are separate. Chat import starts off and needs its own direct-user opt-in; Update preserves that choice.

Selecting Memory waits for a safe fresh context with native memory generation, use and the memories feature disabled. Cold resume retains this policy. Native compaction, history, unrelated Agents and shared defaults retain their owners. The account bridge admits only disabled `thread/memoryMode/set` writes, not general memory enablement or reset. See [Runtime architecture](../architecture/runtime-architecture.md#components-and-app-updates) for service and storage ownership.

| Surface | Responsibility | Boundary |
| --- | --- | --- |
| Shared Hooks | Review current content, trust its hash, enable or disable | Trust and enablement are separate; other tasks using the same file can be affected |
| Connector tool exposure | Edit native `omit_tools_from` preferences for supported surfaces | Inherit, explicit empty and exclusions differ; server restrictions still apply |

The local API has no connector approval/reviewer configuration commands. Pending native approvals and their exact request identities remain supported. Configure account-specific connection policy in Codex.

These retained writes use the native configuration owner and the shared receipt journal. Save once and read back. Unknown outcomes stay unknown until matching evidence resolves them. These controls do not provide a generic Hook script editor or a new connector authentication engine.

## Select native skills

The composer skill picker reads enabled native skills for the selected workspace and account. It filters by name or description before limiting the displayed list, and inserts the exact skill name and absolute path as an input attachment. Disabled skills are not offered. Selection does not install a plugin, change per-account configuration or send a turn.

For shared host skill directories, set `DECODEX_SKILL_ROOTS` before starting the service. It is a platform path list, with colon separators on macOS. Each entry must be an absolute UTF-8 path. The service applies these roots through native `skills/extraRoots/set` on each fresh retained Agent process. Restart the service after changing the environment. An unset variable leaves native discovery unchanged; an explicit empty value sets an empty extra-root list. Codex owns discovery and watching. This setting does not replace per-account plugin setup.

## Retired embedded HTML viewer

Decodex no longer advertises the MCP UI extension or hosts embedded HTML widgets. The widget callback execution path is removed. Ordinary MCP execution, text, attachments, native form prompts and historical receipts remain separate capabilities. A retained receipt is history, not an executable widget.

## Implementation and verification

`agent_integrations` independently projects native inventories. The account-owned bridge admits observation methods while rejecting retired management methods. GPUI presents the result through Tool status. A discovery error must remain distinct from an empty inventory.

See [model/settings ownership](../workflows/models-and-settings.md), [approval rules](../workflows/approvals-and-native-ownership.md) and [product scope](../decisions/upstream-product-scope.md).
