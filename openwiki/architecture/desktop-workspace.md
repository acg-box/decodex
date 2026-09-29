---
type: Reference
title: "Desktop workspace and native glass"
description: "Desktop workspace and native glass"
tags: ["decodex", "architecture"]
sources:
  - id: openwiki-source-6512b631b67649d924c16ba3
    resource: repo://apps/decodex-gpui/src/agent_archive.rs
  - id: openwiki-source-ec2c431b14759817413ba09e
    resource: repo://apps/decodex-gpui/src/agent_graph.rs
  - id: openwiki-source-2ee1164d01a09b38a208f1ac
    resource: repo://apps/decodex-gpui/src/agent_markdown.rs
  - id: openwiki-source-ae2ce5cda718ede63f50be9b
    resource: repo://apps/decodex-gpui/src/agent_native_composer.rs
  - id: openwiki-source-77f7f96f348908c8779b5df6
    resource: repo://apps/decodex-gpui/src/agent_selectable_text.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-a1a71f71175b6cac3a5f1346
    resource: repo://apps/decodex-gpui/src/native_glass_panel.rs
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.6.1
    at: 2026-09-29T20:08:07.145Z
---


# Desktop workspace and native glass

## Information hierarchy

The Agent conversation is the primary workspace. The left sidebar selects the overview and projects; the right agent tree describes ownership; the bottom graph describes work dependencies and reports. A historical Program/Factory graph is not the current desktop surface.

Global shortcuts in `shell.rs` are Command-E for the left sidebar, Command-B for the inspector/agent side, and Command-J for the graph. Window controls stay in the global shell. Settings is a separate presentation with bounded scrolling and shared spacing tokens.

## Material and focus ownership

`window_material.rs` selects Regular or Clear material. macOS uses native Liquid Glass when supported; the fallback requests GPUI platform blur. Actual transparency depends on platform compositor support. Reduced-transparency preferences and `DECODEX_DISABLE_LIQUID_GLASS` prevent the small native-glass panels.

`native_glass_panel.rs` bridges composition only. GPUI still owns UI state, layout and event handlers. The composer is an attached child window created by the main workspace, not another application or service. Settings must not create one. The workspace remains the main window while an attached control can own keyboard focus. Animation scheduling uses the key child's display cadence to avoid throttling a focused composer's parent.

The composer is unavailable for archived or blocked conversations and certain detail/full-screen states. Reopening includes a short layout-settling interval; failures fall back to GPUI presentation. Notification placement and opacity belong to the same native-panel composition boundary.

## Reading and input

Markdown renders native text, code, lists, and links. Text selection and clipboard operations are read-only. Selection currently belongs to each rendered text block; this is not one continuous selection across all messages. Copy controls show short success feedback. Duration and abbreviated token counts remain together at the end of each answer; hovering this row shows detailed usage without changing transcript height.

The history rail follows the reading position. Expanding connection details must preserve the anchor. A centered jump-to-latest control represents ongoing work while the user reads older messages. Input controls expose attachments, delivery policy, model/effort, microphone, and live voice without moving execution authority into the UI.

## State and errors

Ordinary operation feedback goes to the notification center. A conversation that cannot send displays its reason in that conversation and hides the composer. Archived history has an explicit Unarchive control. Background archive-read failures preserve the last confirmed state. Explicit failed checks show local feedback.

## Verification

Use GPUI tests in `shell.rs`, `agent_workspace.rs`, `agent_activity.rs`, `agent_markdown.rs`, `agent_archive.rs` and `settings_surface.rs`. Real macOS acceptance must also check focus, typing, scrolling, panel transitions, and transparency in the signed app. A white or missing automation screenshot alone is not evidence that the user sees a blank window.

See [Conversation presentation and motion](conversation-presentation.md), [Agent coordination](chief-coordination.md) and [Commands and validation](../operations/commands-and-validation.md).

## Retained presentation and drafts

The composer preserves editable drafts and recovered copies. Rich Markdown retains math, Mermaid, weather and source-copy presentation without replacing native execution. The embedded MCP HTML viewer is retired. Ordinary tools and native form prompts remain available. See [conversations](../workflows/conversations-and-recovery.md), [tools](../integrations/tools-plugins-and-apps.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).
