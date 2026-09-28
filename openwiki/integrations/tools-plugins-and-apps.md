---
type: Reference
tags: [decodex, architecture]
title: "MCP tools, plugins and App controls"
description: "Native integration discovery, task plugins, shared Hook and App settings, and constrained MCP widgets."
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T02:19:36.307Z
sources:
  - id: openwiki-source-08ce10c44b7d18a074304e2e
    resource: repo://apps/decodex-gpui/src/agent_hooks.rs
  - id: openwiki-source-f65332f3fa017275f2a56d06
    resource: repo://crates/decodex-codex/src/app_server_client/thread_plugins.rs
  - id: openwiki-source-36058a717845472f94f1d12a
    resource: repo://crates/decodex-runtime/src/agent_app_ui_call.rs
  - id: openwiki-source-e9d609e612bb7e44111ec4b1
    resource: repo://crates/decodex-runtime/src/agent_integrations.rs
  - id: openwiki-source-c8ef79cb2ba8fb6c42ddec90
    resource: repo://docs/archive/upstream-2026-09/mcp-app-ui.md
  - id: openwiki-source-7f71d3e5ee61fd9e0bed12d4
    resource: repo://docs/archive/upstream-2026-09/thread-plugin-selection.md
generated: { by: "codex", at: "2026-09-28T02:19:36.307Z" }
---


# MCP tools, plugins and App controls

## Native discovery and execution

The service reads MCP status, installed plugins and Apps independently. It binds results to the selected native thread, directory and settings guard. Unavailable, unsupported, empty and over-capacity results have different meanings. Ordinary inventory reads do not force a refresh or authorize installation.

Codex owns native tool discovery, filtering, plugin activation and execution. Decodex must not create a second MCP connection or a local tool-policy engine to imitate native behavior. Exact server and plugin names are identities, not display aliases.

## Task plugins (O12)

The task selector edits the native task's exclusion list. It does not uninstall plugins or change shared installation state. Native activation applies to a subsequent admitted turn; a saved setting does not immediately replace the current turn's tool environment. The local-native fixture demonstrates exclusion and restoration of a test MCP tool, not every external connector or Hook.

## Three independent settings surfaces (O13)

| Surface | Responsibility | Boundary |
| --- | --- | --- |
| Shared Hooks | Review current content, trust its hash, enable or disable | Trust and enablement are separate; other tasks using the same file can be affected |
| App connection policy | Show saved/effective approval and reviewer settings | Saving does not answer the pending request or prove effective policy changed |
| Connector tool exposure | Edit native `omit_tools_from` preferences for supported surfaces | Inherit, explicit empty and exclusions differ; server restrictions still apply |

All writes use the native configuration owner and the shared receipt journal. Save once and read back. Unknown outcomes stay unknown until matching evidence resolves them. These controls do not provide a generic Hook script editor or a new connector authentication engine.

## MCP App widgets (O04)

A timeline tool item with a captured UI resource URI can expose **Open app**. The service loads the exact originating document; a native WebKit panel hosts it. A widget tool request displays its source, tool and arguments. **Allow this call** dispatches one reviewed call; Cancel declines it. The outcome is saved before acknowledgment and remains readable after the widget closes.

The host is not a general browser. It has no persistent browser session, camera, microphone, geolocation, file dialogs, popups or unrestricted navigation. External nested frames and some embed flows are unavailable. Native event-stream subscriptions and discovery for historical items without captured resource URIs are not implemented. Keep these limits visible.

## Other retained operations

Task resources/references, archive restoration and explicit installation suggestions remain separate controls. An installation suggestion is not automatic installation. Source-bound native request identity remains required for OAuth forms and permissions.

Representative owners are `agent_integrations`, `agent_app_ui_call`, `agent_app_ui_receipt`, `agent/timeline/app_ui`, the native `app_ui` adapter, `agent_hooks` and the Swift `McpAppHost`. See [model/settings ownership](../workflows/models-and-settings.md) and [approval rules](../workflows/approvals-and-native-ownership.md).
