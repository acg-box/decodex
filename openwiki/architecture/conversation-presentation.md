---
type: Architecture
title: Conversation presentation and motion
description: How Decodex keeps final answers readable, retains work details, and renders loading and floating controls without disturbing the transcript.
tags: [desktop, conversation, motion]
sources:
  - id: openwiki-source-15d320ea458ddf705d950ba9
    resource: repo://apps/decodex-gpui/src/agent_response_metrics.rs
  - id: openwiki-source-b76859cf2790cc53340f082d
    resource: repo://apps/decodex-gpui/src/agent_timeline_groups.rs
  - id: openwiki-source-246a7882a46c2cac4571760d
    resource: repo://apps/decodex-gpui/src/ui_loading.rs
  - id: openwiki-source-afbf2d30c0979a844373d8e5
    resource: repo://apps/decodex-gpui/src/ui_motion.rs
generated: { by: "codex", at: "2026-09-29T06:24:17.023Z" }
verified:
  - by: openwiki/0.6.0
    at: 2026-09-29T09:20:11.292Z
---

# Conversation presentation and motion

The transcript is the primary reading surface. Intermediate work, response statistics and controls support it without replacing the final answer. The UI derives these views from recorded events; collapsing a group does not delete its evidence.

## Progressive disclosure

`agent_timeline_groups` folds process items only after a successful completed turn has a nonempty, explicitly marked final answer. It groups reasoning, plans, commentary and finished tool activity. Failed or interrupted turns and answers with unknown phases remain visible. Attachments and interactive items remain outside the folded group.

Interleaved user input remains in source order. A native turn can contain several user messages, including steering input; grouping uses the native turn identity rather than assuming that each input starts another turn. The disclosure shows a count of earlier messages and lets the reader inspect the underlying process.

Response metadata stays compact at the end of the answer: duration and abbreviated input/output amounts. Hover reveals grouped details in a window-anchored overlay. It does not insert a large block into the transcript. Copy controls are separate actions, not part of the statistics disclosure.

## Loading and retained content

A first conversation read reserves a centered reading surface. Its shape is deliberately neutral: history has not arrived, so the placeholder does not invent message bubbles or text lengths. Compact loading feedback uses a status role and respects reduced motion. Existing content can remain readable during refresh instead of repeatedly replacing the page with a loading state.

## Floating controls and motion

The shared GPUI popover is currently a fixed-anchor opaque surface. Its background, shadow and content are shown together. It does not apply separate primitive fades or slide an already opaque card; those effects previously produced a visible dark surface before or after the content. This is a deliberate fallback, not a claim that fully composited transitions are implemented.

Animation requests use the workspace frame owner on macOS and the normal GPUI animation-frame path elsewhere. Native glass surfaces have their own host integration. Keep these ownership boundaries intact when changing a disclosure: transcript expansion must not recreate or change the composer's material.

## Change and verify

Use the grouping tests for turn boundaries, explicit final answers and interleaved input. Use an actual desktop session for alignment, tool-detail expansion, floating-surface clipping, scrolling and reduced motion. Compilation and event tests do not establish visual smoothness.

See [Desktop workspace](desktop-workspace.md), [Conversations and recovery](../workflows/conversations-and-recovery.md) and [Commands and validation](../operations/commands-and-validation.md).
