---
type: Architecture
title: Conversation presentation and motion
description: Bounded chat layout, grouped voice history, incremental loading, and adaptive streamed text in the native desktop.
tags: [desktop, conversation, motion]
sources:
  - id: openwiki-source-ca9ad15cc8ba4a0657d1c24d
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/BrandLoading.swift
  - id: openwiki-source-bf03f317e64cfefd11f6ea83
    resource: repo://apps/decodex-gpui/src/agent_attached_branches.rs
  - id: openwiki-source-48758ba7d154c82885281d2c
    resource: repo://apps/decodex-gpui/src/agent_composer.rs
  - id: openwiki-source-0a61b25348131fe1bb6e1d12
    resource: repo://apps/decodex-gpui/src/agent_context_settings_wire_tests.rs
  - id: openwiki-source-c76550e85b25876b339f9255
    resource: repo://apps/decodex-gpui/src/agent_context_settings.rs
  - id: openwiki-source-d56ea7499f2ed784b1b6c970
    resource: repo://apps/decodex-gpui/src/agent_conversation_tab_tests.rs
  - id: openwiki-source-1bf529ea3f1649ac9a28334a
    resource: repo://apps/decodex-gpui/src/agent_conversation_tabs.rs
  - id: openwiki-source-d701c906e7fcc9ab2d0917bc
    resource: repo://apps/decodex-gpui/src/agent_inspection.rs
  - id: openwiki-source-85ab533d66ee904d83d8a21a
    resource: repo://apps/decodex-gpui/src/agent_item_context_tests.rs
  - id: openwiki-source-7aecb0d94903aac848b2cdfd
    resource: repo://apps/decodex-gpui/src/agent_item_context.rs
  - id: openwiki-source-0afccc244c69eb8da96d1fa9
    resource: repo://apps/decodex-gpui/src/agent_link_preview.rs
  - id: openwiki-source-9cda3120e5cf6c9c303ec1d9
    resource: repo://apps/decodex-gpui/src/agent_markdown_selection_tests.rs
  - id: openwiki-source-2ee1164d01a09b38a208f1ac
    resource: repo://apps/decodex-gpui/src/agent_markdown.rs
  - id: openwiki-source-711411d5917ac189d9d28e37
    resource: repo://apps/decodex-gpui/src/agent_math_tests.rs
  - id: openwiki-source-d69026af05a9b3138e4656f4
    resource: repo://apps/decodex-gpui/src/agent_math.rs
  - id: openwiki-source-3cb0fdba7d0304893cb4f3a3
    resource: repo://apps/decodex-gpui/src/agent_math/render/structured.rs
  - id: openwiki-source-bbf48964e1bd60f04ecb7f62
    resource: repo://apps/decodex-gpui/src/agent_mermaid_tests.rs
  - id: openwiki-source-4dfe438694434f1f2e34b2f9
    resource: repo://apps/decodex-gpui/src/agent_mermaid_view.rs
  - id: openwiki-source-b223bcac929a26e81aa4ef37
    resource: repo://apps/decodex-gpui/src/agent_mermaid.rs
  - id: openwiki-source-cdf91143ee7350a723ed596e
    resource: repo://apps/decodex-gpui/src/agent_mermaid/tests.rs
  - id: openwiki-source-ae2ce5cda718ede63f50be9b
    resource: repo://apps/decodex-gpui/src/agent_native_composer.rs
  - id: openwiki-source-0eea3c2bab20e4a9d8e3d80a
    resource: repo://apps/decodex-gpui/src/agent_prompt_confirm_wire_tests.rs
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-56b561d7e6dd906d218aa9d1
    resource: repo://apps/decodex-gpui/src/agent_read_state.rs
  - id: openwiki-source-15d320ea458ddf705d950ba9
    resource: repo://apps/decodex-gpui/src/agent_response_metrics.rs
  - id: openwiki-source-309d7ec654ab4d531c906212
    resource: repo://apps/decodex-gpui/src/agent_sample_ui.rs
  - id: openwiki-source-912dbe8f46e7ec6026887bbe
    resource: repo://apps/decodex-gpui/src/agent_sample.rs
  - id: openwiki-source-77f7f96f348908c8779b5df6
    resource: repo://apps/decodex-gpui/src/agent_selectable_text.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-04e81f339ea9d70e791f5b0e
    resource: repo://apps/decodex-gpui/src/agent_text_link_hover_tests.rs
  - id: openwiki-source-afe7c47e476800002dba8df3
    resource: repo://apps/decodex-gpui/src/agent_text_links.rs
  - id: openwiki-source-4247deee56f626d37a52d5b0
    resource: repo://apps/decodex-gpui/src/agent_text_reveal.rs
  - id: openwiki-source-2fb9e466c6aafdf4f4950bc1
    resource: repo://apps/decodex-gpui/src/agent_timeline_find.rs
  - id: openwiki-source-b76859cf2790cc53340f082d
    resource: repo://apps/decodex-gpui/src/agent_timeline_groups.rs
  - id: openwiki-source-d57edbe5de23ebee985fc88b
    resource: repo://apps/decodex-gpui/src/agent_timeline_inputs.rs
  - id: openwiki-source-1849030b658783d8ea0855b0
    resource: repo://apps/decodex-gpui/src/agent_timeline_receipts.rs
  - id: openwiki-source-73d67d21d29d49e7272436e0
    resource: repo://apps/decodex-gpui/src/agent_timeline_render.rs
  - id: openwiki-source-2da3727221b66d60f1a2e9ef
    resource: repo://apps/decodex-gpui/src/agent_timeline_scroll.rs
  - id: openwiki-source-691432bb61082c358b6f9c24
    resource: repo://apps/decodex-gpui/src/agent_timeline.rs
  - id: openwiki-source-40c9c6a55485557343853517
    resource: repo://apps/decodex-gpui/src/agent_usage_estimates.rs
  - id: openwiki-source-32bb36aae256aa57e6e82bd0
    resource: repo://apps/decodex-gpui/src/agent_voice.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-7b89f5bc6ecd604ca4e97dc2
    resource: repo://apps/decodex-gpui/src/composer_text.rs
  - id: openwiki-source-fca3688e96d92c09b7ba015f
    resource: repo://apps/decodex-gpui/src/native_material_panel.rs
  - id: openwiki-source-592957b8edd8a689f738c189
    resource: repo://apps/decodex-gpui/src/ui_frame.rs
  - id: openwiki-source-246a7882a46c2cac4571760d
    resource: repo://apps/decodex-gpui/src/ui_loading.rs
  - id: openwiki-source-2316e3cffd10487b54cd63f4
    resource: repo://apps/decodex-gpui/src/ui_material.rs
  - id: openwiki-source-afbf2d30c0979a844373d8e5
    resource: repo://apps/decodex-gpui/src/ui_motion.rs
  - id: openwiki-source-79f9e926ceb3e6b2643c73a5
    resource: repo://apps/decodex-gpui/src/ui_native_popover.rs
  - id: openwiki-source-1b1f8653954117c3fd5fc77c
    resource: repo://apps/decodex-gpui/src/ui_palette.rs
  - id: openwiki-source-23959670579000a30234423d
    resource: repo://apps/decodex-gpui/src/ui_text_reveal.rs
  - id: openwiki-source-570564020f98df7a4d6d0b61
    resource: repo://apps/decodex-gpui/src/ui_theme.rs
  - id: openwiki-source-343d35c489d39a7a672412a2
    resource: repo://apps/decodex-gpui/src/ui_working.rs
  - id: openwiki-source-d6e2f0d0037f3d6ddcd4f6ac
    resource: repo://apps/decodex-gpui/src/voice_conversation_groups.rs
  - id: openwiki-source-651d1fb6c9e49916a916ab51
    resource: repo://Cargo.toml
  - id: openwiki-source-2b08587e9b26e203156110c8
    resource: repo://crates/decodex-codex/src/app_server_client/context_preferences.rs
  - id: openwiki-source-de4d0cc1dfd69783c61c375f
    resource: repo://crates/decodex-codex/src/app_server_client/observed_timing.rs
  - id: openwiki-source-c01ab8da738707861c64c143
    resource: repo://crates/decodex-protocol/src/desktop_message_delivery.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-4a97944ff4c701a054e27291
    resource: repo://crates/decodex-runtime/src/agent/activity.rs
  - id: openwiki-source-64d7588c1af50398568a792e
    resource: repo://crates/decodex-runtime/src/agent/timeline.rs
  - id: openwiki-source-6b78c4fcf18e4d7a81d69679
    resource: repo://crates/decodex-runtime/src/agent/timeline/attachments.rs
  - id: openwiki-source-cb57a61e719abcde2e5a7754
    resource: repo://crates/decodex-runtime/src/agent/timeline/media_tests.rs
  - id: openwiki-source-8d3e8e016f038e821f68dd0c
    resource: repo://crates/decodex-runtime/src/agent/timeline/media.rs
  - id: openwiki-source-74145d84eb83d80ec3062595
    resource: repo://crates/decodex-runtime/src/agent/timeline/metrics.rs
  - id: openwiki-source-6197a876b2effbe992b1e2ec
    resource: repo://database/src/agent.rs
generated: { by: "codex", at: "2026-10-31T13:17:02.524Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-31T13:17:02.524Z
---


# Conversation presentation and motion

The transcript is the primary reading surface. Intermediate work, response statistics and controls support it without replacing the final answer. The UI derives these views from recorded events; collapsing a group does not delete its evidence.

## Progressive disclosure

`agent_timeline_groups` folds process items only after a successful completed turn has a nonempty, explicitly marked final answer. It groups reasoning, plans, commentary and finished tool activity. Failed or interrupted turns and answers with unknown phases remain visible. Attachments and interactive items remain outside the folded group.

Interleaved user input remains in source order. A native turn can contain several user messages, including steering input; grouping uses the native turn identity rather than assuming that each input starts another turn. The disclosure shows a count of earlier messages and lets the reader inspect the underlying process.

Response metadata stays compact at the end of the answer: duration and abbreviated input/output amounts. Hover reveals grouped details in a window-anchored overlay. It does not insert a large block into the transcript. Copy controls are separate actions, not part of the statistics disclosure.

A completed response also has a question action on hover or keyboard focus. Selected response text offers **Ask about selection**, and the composer toolbar provides **Ask separately** without a quote. All three use the same bounded, one-shot sampler and a shared anchored popover. The question and answer stay outside the transcript, and opening the popover preserves the main draft and reading position. Selection callbacks carry the visible excerpt and reject a replaced source; native sampling still owns execution and cancellation. See [separate questions](../workflows/conversations-and-recovery.md#ask-a-separate-question).

A collapsed command row shows the number of native-retained output lines when that count is available. Opening the output removes the count label. The count excludes command text and discarded output; it does not claim terminal wrapping or recovery of truncated bytes.

The composer shows a reported dollar amount beside **Context**. A native amount uses `$`; an estimated amount uses `≈ $`. Missing dollar data stays hidden. The source-bound read refreshes when the conversation becomes idle after an update; an unchanged idle conversation does not poll billing. See [usage estimates](../workflows/models-and-settings.md#conversation-cost-estimates) for the Menu Bar view and coverage limits.

### Observed turn activity

The response details can show observed model output, context compaction, tool activity, automatic review and blocking user requests. The native transport records the start and end of these activities with a monotonic clock. It combines overlapping intervals within each category. Categories can overlap each other, so their values do not add up to the turn duration. Model output is the visible output or reasoning item lifetime; it is not the full inference time.

The service saves a completed observation with the exact Work, thread and turn identities. Timeline reads use those identities to retrieve it. A historical activity can also use a valid native item duration or an exact saved activity duration when available. Missing turn starts, unmatched boundaries, overflow and unavailable measurements stay **Unknown**. A measured zero stays zero. Full inference, runtime overhead and other unobserved time remain unknown; these partial observations are not a complete native critical path.

## Loading and retained content

A first conversation read reserves a centered reading surface. Its shape is deliberately neutral: history has not arrived, so the placeholder does not invent message bubbles or text lengths. Compact loading feedback uses a status role and respects reduced motion. Existing content can remain readable during refresh instead of repeatedly replacing the page with a loading state.

The shared work and loading components use the approved Decodex cloud mark. The welcome view uses the same mark for its entrance. Menu-bar account, profile and sign-in loading reuse the packaged status icon through one SwiftUI component. The existing loading delays and retained-content transitions still apply. Working text uses a color-only sweep; reduced motion keeps the mark and text static.

## Find in retained history

Use **Cmd-F** or the conversation header's search control. Find includes text inside folded activity and the retained live reply. It temporarily opens the current matching process, wait or voice group and leaves manual disclosure state unchanged. The current matched native text revision stays readable through live refresh and resize.

**Previous**, **Next** and Enter move between matching records. Escape closes Find. The query owns Enter while it is open, so repeated Enter does not send the message draft. A blue edge and a matching excerpt identify the current record. The count covers records in the loaded history window, not every occurrence in the complete native store.

**Load earlier and search** uses the existing older-history cursor and bounded window. The UI states its loaded-history coverage. Shortened output and unavailable full tool output are not completely searched. Summary-only history does not offer a complete-search claim. See [conversation recovery](../workflows/conversations-and-recovery.md) and `retained_desktop_feedback.find` in the [contract catalog](../../automations/upstream/retained-contracts.json).

## Locate a known native item

Choose **Show in conversation** in an existing tool detail to locate its exact native item. A loaded item scrolls into view and its process group opens temporarily without another history query. Ordinary messages have no nearby-history button.

For an unloaded item, the main transcript displays a bounded same-turn native history window through the existing item reader. **Earlier messages** and **Later messages** replace that window through native cursors. A fixed **Return to conversation** action stays below the conversation header and restores the earlier reading position. The composer draft stays intact. Failed page reads retain the current window.

Item pages have no canonical timeline ordinals. They remain separate from the cached ordinary page and use exact thread, turn and item identities for presentation anchors. Background history refresh does not replace an active location. Find and Latest return to the ordinary conversation. See [history navigation](../workflows/conversations-and-recovery.md#read-items-near-a-known-message).

## Side chat tabs

The conversation header contains the source conversation and its open Side chats. Each tab shows the existing activity state. The tab strip scrolls horizontally and brings the selected tab into view. The adjacent **+** action reviews a branch from the source conversation's latest completed turn. **Create side chat** confirms that review; it does not send a message.

A Side chat uses the existing native attached fork. Its visible history starts empty, while its model context includes the reviewed source prefix. Later messages in the source do not automatically enter the Side chat. Each conversation keeps its own draft and history position through the existing workspace navigation.

Only confirmed native attachments with matching source, target and runtime identities belong to the tab family. A local parent link is a lookup hint, not evidence of an attachment. The workspace owns this attachment cache independently of the prompt editor and clears it when the relevant source or runtime changes. Current-family Side chats do not also appear as duplicate sidebar rows.

Close hides a tab and returns to its exact source conversation. It does not archive or delete the native branch. Use **Agent preferences → Attached branches** to reopen it. The source remains available in the tab strip.

## Conversation layout

The composer and ordinary transcript share an 880-point maximum width with 16-point side insets. Narrow windows use the available width. User messages remain narrower and right-aligned. The history rail and composer use the same animated inset so their content edges stay aligned when the rail opens or closes. Native child-agent conversations use the same width tokens.

The composer sits over the bottom of the conversation. Its measured height supplies bottom padding to the scroll content, so the final rows can move above the composer. The editor grows to six lines, then scrolls its text internally. Its action row remains below the text. In the conversation that owns the call, Live replaces the text editor slot with a compact waveform. Other conversations retain their own text editors. A background call bar identifies the owner and provides Return, Mute and End controls.

## Voice history and pagination

A voice call has one expandable group with speaker-labelled transcript text. Call occurrences remain distinct even when a session ID is reused. An overlapping final transcript tail is merged into the recorded utterances instead of becoming another chat message. Ordinary typed messages stay outside the voice group.

Completed calls with no transcript and no failure are hidden from history and its navigation rail. Failed or pending empty calls retain a compact status. A completed call uses its closing boundary for both the disclosure identity and the outer history anchor. Loading earlier records therefore preserves an already-expanded call.

The first native page requests up to 30 records. Earlier pages request up to 15 and can retry smaller sizes when a page is too dense. Records are not conversation rounds: grouping and hidden items affect how many rows appear. Automatic prefetch waits for anchor restoration. It can fill an underfilled viewport; once content fills the viewport, further prefetch requires upward-scroll intent.

Submitted input has message delivery state separate from the current editor. A positively unsent message offers **Resend**. An unconfirmed message offers a status read, not another send. Resending keeps the original operation and does not replace later text in the input bar. No saved-copy counter or profile-snapshot manager appears in the conversation.

An unsent input uses the normal right-aligned user bubble at the end of the transcript. Its full text wraps and scrolls with the conversation; it is not pinned above the input bar. One compact row below it aligns **Not sent** and, when available, **Resend**. Hover over the status to inspect the reason. There is no separate error panel, disclosure header, copy action or dismiss action. Archived conversations do not show retry accessories.

Saved edit recovery and history refresh/source controls live in Details. Failed input remains an ordinary message in the transcript. Active editing and unresolved-operation notices also use the chat flow. There is no permanent list of saved drafts. See [conversation recovery](../workflows/conversations-and-recovery.md) for edit and Fork ownership.

After a native provider mismatch, a read-only notice identifies the original and current providers. **Create branch…** opens the existing branch review. Its confirmation identifies the destination provider and explains that a later send shares the inherited context. Creation sends no input, and cancellation retains the original draft. Ordinary branches and Side chats keep their source provider.

## Prose links and formulas

HTTP and HTTPS URLs in prose are clickable in messages, question titles, approval explanations, MCP descriptions and feedback. Link ranges preserve the complete destination, including Unicode paths and balanced parentheses. Code spans and blocks stay literal; explicit Markdown links keep their authored destinations. Selecting text does not open a link.

Selectable text shows a hand pointer over an actionable link. Ordinary text, empty space and unsupported destinations keep the text cursor. Hover and click use the current shaped text positions. Each paint recomputes the hover target, so reflow and scrolling do not retain an old link position. Drag selection and copy keep their existing behavior.

The bounded Unicode math renderer supports aligned equations, boxed models, grouped scripts, parenthesized matrices, display annotations and inline zero. Unsupported notation and layouts that exceed the available width retain their TeX source. Conversion is limited to 4,096 input bytes, 16 rows and 256 columns, with a shared parser-depth limit. Copy retains the original source.

## Generated image attachments

The runtime keeps a generated image attachment when the native item contains a `fileId`. It uses the existing Stored attachment description and does not copy that identity into public history fields. A file reference takes precedence over a transitional `result`, including nonempty stale bytes.

The media reader currently supports inline generated image bytes. It reports file references as unsupported. It does not interpret `savedPath` as a service-host file because that path can belong to another execution environment. The regression tests cover file-only and transitional records, inline bytes, and same-named host paths. These checks do not establish live image generation or desktop visual acceptance.

## Conversation read status

Task preferences shows the native read receipt for an eligible root conversation. Reading history or refreshing the receipt does not acknowledge a result. Mark as read and Mark as unread submit the displayed revision with a source-bound review token. A replaced source discards the observation; a rejected or uncertain write refreshes status without replay. Missing receipts remain unavailable. Native read state stays separate from Dock handoff and attention state.

## Context defaults

Task preferences includes a Context defaults panel for background compaction and retention of delegation instructions. The panel reads the native effective project default and the saved user value. Its On and Off actions write the same native configuration file that manual config edits use. An override remains visible as a different effective project default.

These defaults apply to new conversations. Saving them does not restart a loaded conversation or claim that an in-progress task has switched behavior. An unconfirmed save triggers a fresh read and requires another explicit choice before a new write. See [Runtime architecture](runtime-architecture.md#context-defaults) for the configuration keys and native limits.

## Floating controls and motion

Lazy shared popovers use one native material host per parent window when that host is available. The host places and focuses its content outside the transcript layout. The GPUI fallback moves and fades the complete surface by four points, with its background, shadow and content together. It does not animate the height or fade primitives separately.

Presentation entities request GPUI animation frames while application policy permits animation. Keyboard focus does not set the cadence of sibling windows. Native glass surfaces have their own host integration. Transcript expansion must not recreate or change the composer's material.

Window and input-bar materials are independent preferences. Dark Liquid Glass uses shared surface opacity across color themes; Decodex does not add an opaque Graph or composer cover. Thin retains its separate matte palette. The native input bar keeps its focused and unfocused glass styles.

Streamed model output, dictation and Live captions share a grapheme-safe text reveal. Model output uses an adaptive reserve across network gaps, with a 300–700 ms receive horizon. Completion limits the remaining reveal deadline to 160 ms. Dictation and Live captions keep a 120 ms response horizon. Corrections replace the visible prefix without replay. Reduced motion reveals current text directly. These are presentation policies, not transport-latency or microphone-readiness guarantees.

While a rich response streams, an unfinished Markdown link keeps its label visible and holds back the destination. A closing parenthesis, a newline or finalization restores normal source rendering. Code and stored response text are unaffected.

## Mermaid diagrams

The pinned Ondine compiler owns Mermaid syntax and layout. Decodex adapts its SVG theme and rasterizes the result through GPUI. The desktop integration suite includes 33 diagram-family fixtures and examples with stadium nodes, Unicode labels and authored line breaks. The retired local parser and its 40-cell label limit do not define the current renderer.

Desktop options disable image resources and interactive links. Compilation limits edges and SVG output, and the desktop rejects an excessive decoded canvas before font parsing or image allocation. These limits bound diagram preparation; they do not measure whole-frame performance.

Preparation runs in the background. An application cache keyed by source and selected color scheme retains at most 64 images within a 64 MiB byte budget. Virtualized rows can reuse a prepared image. A small geometry record preserves row height while an evicted image is restored. A change to the diagram source or color scheme selects new state; later prose in the same reply does not invalidate an unchanged diagram.

Incomplete fences and failed compilation retain the literal source. Wide diagrams scroll without wrapping or changing image geometry. Copy Mermaid source preserves the original tags, quotes and line endings. The GPUI regression tests cover source copy, a narrow viewport, row culling and cache restoration. These tests are distinct from installed-app scrolling or frame-rate acceptance.

## Change and verify

Use the grouping tests for turn boundaries, explicit final answers and interleaved input. Use an actual desktop session for alignment, tool-detail expansion, floating-surface clipping, scrolling and reduced motion. Compilation and event tests do not establish visual smoothness.

See [Desktop workspace](desktop-workspace.md), [Conversations and recovery](../workflows/conversations-and-recovery.md) and [Commands and validation](../operations/commands-and-validation.md).
