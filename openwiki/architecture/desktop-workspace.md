---
type: Reference
title: "Desktop workspace and native glass"
description: "Desktop workspace and native glass"
tags: ["decodex", "architecture"]
verified:
  - by: openwiki/0.4.3
    at: 2026-09-22T05:36:11.119Z
sources:
  - id: openwiki-source-ed0b57b7a04c5fc4bc39b55b
    resource: repo://apps/decodex-gpui/src/chief_archive.rs
  - id: openwiki-source-477d041b92b25547bc39e55d
    resource: repo://apps/decodex-gpui/src/chief_graph.rs
  - id: openwiki-source-db29fbf14600d581ea3469e0
    resource: repo://apps/decodex-gpui/src/chief_markdown.rs
  - id: openwiki-source-d0fed23c6c28ca7a0ab936fa
    resource: repo://apps/decodex-gpui/src/chief_native_composer.rs
  - id: openwiki-source-dded6d228983ca3f173f14ba
    resource: repo://apps/decodex-gpui/src/chief_selectable_text.rs
  - id: openwiki-source-a78ea5fe51f1eae9468e41e0
    resource: repo://apps/decodex-gpui/src/chief_tree.rs
  - id: openwiki-source-a1a71f71175b6cac3a5f1346
    resource: repo://apps/decodex-gpui/src/native_glass_panel.rs
generated: { by: "codex", at: "2026-09-22T05:36:11.119Z" }
---

# Desktop workspace and native glass

## Information hierarchy

The Chief conversation is the primary workspace. The left sidebar selects the overview and projects; the right agent tree describes ownership; the bottom graph describes work dependencies and reports. A historical Program/Factory graph is not the current desktop surface.

Global shortcuts in `shell.rs` are Command-E for the left sidebar, Command-B for the inspector/agent side, and Command-J for the graph. Window controls stay in the global shell. Settings is a separate presentation with bounded scrolling and shared spacing tokens.

## Material and focus ownership

`window_material.rs` selects Regular or Clear material. macOS uses native Liquid Glass when supported; the fallback requests GPUI platform blur. Actual transparency depends on platform compositor support. Reduced-transparency preferences and `DECODEX_DISABLE_LIQUID_GLASS` prevent the small native-glass panels.

`native_glass_panel.rs` bridges composition only. GPUI still owns UI state, layout and event handlers. The composer is an attached child window created by the main workspace, not another application or service. Settings must not create one. The workspace remains the main window while an attached control can own keyboard focus. Animation scheduling uses the key child's display cadence to avoid throttling a focused composer's parent.

The composer is unavailable for archived or blocked conversations and certain detail/full-screen states. Reopening includes a short layout-settling interval; failures fall back to GPUI presentation. Notification placement and opacity belong to the same native-panel composition boundary.

## Reading and input

Markdown renders native text, code, lists, and links. Text selection and clipboard operations are read-only. Selection currently belongs to each rendered text block; this is not one continuous selection across all messages. Copy controls show short success feedback. Response duration is primary metadata; compact token details are available from its detail affordance.

The history rail follows the reading position. Expanding connection details must preserve the anchor. A centered jump-to-latest control represents ongoing work while the user reads older messages. Input controls expose attachments, delivery policy, model/effort, microphone, and live voice without moving execution authority into the UI.

## State and errors

Ordinary operation feedback goes to the notification center. A conversation that cannot send displays its reason in that conversation and hides the composer. Archived history has an explicit Unarchive control. One transient background archive-read failure does not replace confirmed state or immediately flash an error; repeated failures remain visible.

## Verification

Use GPUI tests in `shell.rs`, `chief_workspace.rs`, `chief_activity.rs`, `chief_markdown.rs`, `chief_archive.rs` and `settings_surface.rs`. Real macOS acceptance must also check focus, typing, scrolling, panel transitions, and transparency in the signed app. A white or missing automation screenshot alone is not evidence that the user sees a blank window.

See [Chief coordination](chief-coordination.md) and [Commands and validation](../operations/commands-and-validation.md).
