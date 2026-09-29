---
type: Reference
title: "Native tools and connection ownership"
description: "Codex-owned plugin setup, enabled skill selection, host skill roots and retired HTML execution."
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T13:52:19.644Z
sources:
  - id: openwiki-source-d700ef551f46158044378d8f
    resource: repo://apps/decodex-cli/src/lib.rs
  - id: openwiki-source-08ce10c44b7d18a074304e2e
    resource: repo://apps/decodex-gpui/src/agent_hooks.rs
  - id: openwiki-source-01379a7fb49ab2d638863891
    resource: repo://apps/decodex-gpui/src/agent_skills.rs
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
  - id: openwiki-source-a09c082db4ad1473c4d1e557
    resource: repo://crates/decodex-runtime/src/application.rs
generated: { by: "codex", at: "2026-09-29T13:52:19.644Z" }
---

# Native tools and connection ownership

## Native discovery and execution

The service reads MCP status, installed plugins and Apps independently. It binds results to the selected native thread, directory and settings guard. Unavailable, unsupported, empty and over-capacity results have different meanings. Ordinary inventory reads do not force a refresh or authorize installation.

Codex owns native tool discovery, filtering, plugin activation and execution. Decodex must not create a second MCP connection or a local tool-policy engine to imitate native behavior. Exact server and plugin names are identities, not display aliases.

## Plugin configuration belongs to Codex

The desktop shows tool status and source ownership. Install and configure plugins and connections in Codex for the selected account. Local task-plugin selection, plugin installation, sync and authentication-management controls are retired. Existing native settings observations and historical operation receipts remain readable; their presence does not imply a current management action.

## Native settings boundaries

| Surface | Responsibility | Boundary |
| --- | --- | --- |
| Shared Hooks | Review current content, trust its hash, enable or disable | Trust and enablement are separate; other tasks using the same file can be affected |
| Connector tool exposure | Edit native `omit_tools_from` preferences for supported surfaces | Inherit, explicit empty and exclusions differ; server restrictions still apply |

Local connector approval/reviewer management is also retired. Pending native approvals and their exact request identities remain supported. Configure account-specific connection policy in Codex.

These retained writes use the native configuration owner and the shared receipt journal. Save once and read back. Unknown outcomes stay unknown until matching evidence resolves them. These controls do not provide a generic Hook script editor or a new connector authentication engine.

## Select native skills

The composer skill picker reads enabled native skills for the selected workspace and account. It filters by name or description before limiting the displayed list, and inserts the exact skill name and absolute path as an input attachment. Disabled skills are not offered. Selection does not install a plugin, change per-account configuration or send a turn.

For shared host skill directories, set `DECODEX_SKILL_ROOTS` before starting the service. It is a platform path list, with colon separators on macOS. Each entry must be an absolute UTF-8 path. The service applies these roots through native `skills/extraRoots/set` on each fresh retained Agent process. Restart the service after changing the environment. An unset variable leaves native discovery unchanged; an explicit empty value sets an empty extra-root list. Codex owns discovery and watching. This setting does not replace per-account plugin setup.

## Retired embedded HTML viewer

Decodex no longer advertises the MCP UI extension or hosts embedded HTML widgets. The widget callback execution path is removed. Ordinary MCP execution, text, attachments, native form prompts and historical receipts remain separate capabilities. A retained receipt is history, not an executable widget.

## Implementation and verification

`agent_integrations` independently projects native inventories. The account-owned bridge admits observation methods while rejecting retired management methods. GPUI presents the result through Tool status. A discovery error must remain distinct from an empty inventory.

See [model/settings ownership](../workflows/models-and-settings.md), [approval rules](../workflows/approvals-and-native-ownership.md) and [product scope](../decisions/upstream-product-scope.md).
