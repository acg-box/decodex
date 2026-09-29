---
type: Reference
tags: [decodex, architecture]
title: "Native tools and connection ownership"
description: "Native tool observations, configuration ownership and the retired embedded HTML boundary."
sources:
  - id: openwiki-source-08ce10c44b7d18a074304e2e
    resource: repo://apps/decodex-gpui/src/agent_hooks.rs
  - id: openwiki-source-a0063c7b07a1bc990ee9af6c
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process.rs
  - id: openwiki-source-e9d609e612bb7e44111ec4b1
    resource: repo://crates/decodex-runtime/src/agent_integrations.rs
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T06:24:17.023Z
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
| App connection policy | Show saved/effective approval and reviewer settings | Saving does not answer the pending request or prove effective policy changed |
| Connector tool exposure | Edit native `omit_tools_from` preferences for supported surfaces | Inherit, explicit empty and exclusions differ; server restrictions still apply |

All writes use the native configuration owner and the shared receipt journal. Save once and read back. Unknown outcomes stay unknown until matching evidence resolves them. These controls do not provide a generic Hook script editor or a new connector authentication engine.

## Retired embedded HTML viewer

Decodex no longer advertises the MCP UI extension or hosts embedded HTML widgets. The widget callback execution path is removed. Ordinary MCP execution, text, attachments, native form prompts and historical receipts remain separate capabilities. A retained receipt is history, not an executable widget.

## Implementation and verification

`agent_integrations` independently projects native inventories. The account-owned bridge admits observation methods while rejecting retired management methods. GPUI presents the result through Tool status. A discovery error must remain distinct from an empty inventory.

See [model/settings ownership](../workflows/models-and-settings.md), [approval rules](../workflows/approvals-and-native-ownership.md) and [product scope](../decisions/upstream-product-scope.md).
