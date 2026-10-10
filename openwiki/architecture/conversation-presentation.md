---
type: Architecture
title: Conversation presentation and motion
description: Bounded chat layout, grouped voice history, incremental loading, and adaptive streamed text in the native desktop.
tags: [desktop, conversation, motion]
sources:
  - id: openwiki-source-d701c906e7fcc9ab2d0917bc
    resource: repo://apps/decodex-gpui/src/agent_inspection.rs
  - id: openwiki-source-4dfe438694434f1f2e34b2f9
    resource: repo://apps/decodex-gpui/src/agent_mermaid_view.rs
  - id: openwiki-source-d626966ee70bffd764516a59
    resource: repo://apps/decodex-gpui/src/agent_mermaid/draw.rs
  - id: openwiki-source-63e4988aff5dd37bc614412f
    resource: repo://apps/decodex-gpui/src/agent_mermaid/parse.rs
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-56b561d7e6dd906d218aa9d1
    resource: repo://apps/decodex-gpui/src/agent_read_state.rs
  - id: openwiki-source-15d320ea458ddf705d950ba9
    resource: repo://apps/decodex-gpui/src/agent_response_metrics.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-4247deee56f626d37a52d5b0
    resource: repo://apps/decodex-gpui/src/agent_text_reveal.rs
  - id: openwiki-source-b76859cf2790cc53340f082d
    resource: repo://apps/decodex-gpui/src/agent_timeline_groups.rs
  - id: openwiki-source-691432bb61082c358b6f9c24
    resource: repo://apps/decodex-gpui/src/agent_timeline.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-7b89f5bc6ecd604ca4e97dc2
    resource: repo://apps/decodex-gpui/src/composer_text.rs
  - id: openwiki-source-246a7882a46c2cac4571760d
    resource: repo://apps/decodex-gpui/src/ui_loading.rs
  - id: openwiki-source-afbf2d30c0979a844373d8e5
    resource: repo://apps/decodex-gpui/src/ui_motion.rs
  - id: openwiki-source-23959670579000a30234423d
    resource: repo://apps/decodex-gpui/src/ui_text_reveal.rs
  - id: openwiki-source-570564020f98df7a4d6d0b61
    resource: repo://apps/decodex-gpui/src/ui_theme.rs
  - id: openwiki-source-d6e2f0d0037f3d6ddcd4f6ac
    resource: repo://apps/decodex-gpui/src/voice_conversation_groups.rs
  - id: openwiki-source-64d7588c1af50398568a792e
    resource: repo://crates/decodex-runtime/src/agent/timeline.rs
generated: { by: "codex", at: "2026-10-10T18:01:25.138Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-10T18:01:25.138Z
---


# Conversation presentation and motion

The transcript is the primary reading surface. Intermediate work, response statistics and controls support it without replacing the final answer. The UI derives these views from recorded events; collapsing a group does not delete its evidence.

## Progressive disclosure

`agent_timeline_groups` folds process items only after a successful completed turn has a nonempty, explicitly marked final answer. It groups reasoning, plans, commentary and finished tool activity. Failed or interrupted turns and answers with unknown phases remain visible. Attachments and interactive items remain outside the folded group.

Interleaved user input remains in source order. A native turn can contain several user messages, including steering input; grouping uses the native turn identity rather than assuming that each input starts another turn. The disclosure shows a count of earlier messages and lets the reader inspect the underlying process.

Response metadata stays compact at the end of the answer: duration and abbreviated input/output amounts. Hover reveals grouped details in a window-anchored overlay. It does not insert a large block into the transcript. Copy controls are separate actions, not part of the statistics disclosure.

## Loading and retained content

A first conversation read reserves a centered reading surface. Its shape is deliberately neutral: history has not arrived, so the placeholder does not invent message bubbles or text lengths. Compact loading feedback uses a status role and respects reduced motion. Existing content can remain readable during refresh instead of repeatedly replacing the page with a loading state.

## Conversation layout

The composer and ordinary transcript share an 880-point maximum width with 16-point side insets. Narrow windows use the available width. User messages remain narrower and right-aligned. The history rail and composer use the same animated inset so their content edges stay aligned when the rail opens or closes. Native child-agent conversations use the same width tokens.

The composer reserves real space below the transcript viewport. History is clipped above that footer instead of extending behind a floating input. The editor grows to six lines, then scrolls its text internally. Its action row remains below the text. Live replaces the text editor slot with a compact waveform and restores the draft when the call ends.

## Voice history and pagination

A voice call has one expandable group with speaker-labelled transcript text. Call occurrences remain distinct even when a session ID is reused. An overlapping final transcript tail is merged into the recorded utterances instead of becoming another chat message. Ordinary typed messages stay outside the voice group.

Completed calls with no transcript and no failure are hidden from history and its navigation rail. Failed or pending empty calls retain a compact status. A completed call uses its closing boundary for both the disclosure identity and the outer history anchor. Loading earlier records therefore preserves an already-expanded call.

The first native page requests up to 30 records. Earlier pages request up to 15 and can retry smaller sizes when a page is too dense. Records are not conversation rounds: grouping and hidden items affect how many rows appear. Automatic prefetch waits for anchor restoration. It can fill an underfilled viewport; once content fills the viewport, further prefetch requires upward-scroll intent.

Saved edit recovery and history refresh/source controls live in Details. The normal transcript contains the active editor or a compact notice for an unresolved operation, rather than a permanent list of saved drafts. See [conversation recovery](../workflows/conversations-and-recovery.md) for edit and Fork ownership.

## Conversation read status

Task preferences shows the native read receipt for an eligible root conversation. Reading history or refreshing the receipt does not acknowledge a result. Mark as read and Mark as unread submit the displayed revision with a source-bound review token. A replaced source discards the observation; a rejected or uncertain write refreshes status without replay. Missing receipts remain unavailable. Native read state stays separate from Dock handoff and attention state.

## Floating controls and motion

The shared GPUI popover moves into place by four points as one opaque surface. Its background, shadow and content move together, without a height clip or separate primitive fades. The menu overlays the page so the layout does not clip its blur and shadow.

Animation requests use the workspace frame owner on macOS and the normal GPUI animation-frame path elsewhere. Native glass surfaces have their own host integration. Keep these ownership boundaries intact when changing a disclosure: transcript expansion must not recreate or change the composer's material.

Streamed model output, dictation and Live captions share a grapheme-safe text reveal. Its rate adapts to the pending text and time remaining, with a 120 ms catch-up target. Corrections replace the visible prefix without replaying it; reduced motion reveals the current text directly. This is a presentation policy, not a guarantee about transport latency or microphone readiness.

## Mermaid flowcharts

The bounded Mermaid renderer supports stadium start/end nodes such as `A([Start])` alongside rectangular and decision nodes. It draws stadium nodes with rounded corners in all four flowchart directions. Repeated declarations must agree on label and shape. Malformed or unsupported diagrams retain the complete source fallback. Wide supported diagrams scroll horizontally, and the copy action retains the original Mermaid source.

## Change and verify

Use the grouping tests for turn boundaries, explicit final answers and interleaved input. Use an actual desktop session for alignment, tool-detail expansion, floating-surface clipping, scrolling and reduced motion. Compilation and event tests do not establish visual smoothness.

See [Desktop workspace](desktop-workspace.md), [Conversations and recovery](../workflows/conversations-and-recovery.md) and [Commands and validation](../operations/commands-and-validation.md).
