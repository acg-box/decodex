---
type: Architecture
title: Interface geometry and motion
description: Reusable layout, activity-island motion, reading spacing and theme rules for the native Decodex desktop.
tags: [desktop, design, motion, layout]
verified:
  - by: openwiki/0.9.0
    at: 2026-11-20T07:13:28.942Z
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-16fcd97a1a349ad4a3709dd7
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/HoverHint.swift
  - id: openwiki-source-51c6a903a86b67bbf46fe288
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardSectionView.swift
  - id: openwiki-source-430d1da3c87d463391edfc47
    resource: repo://apps/decodex-gpui/menubar/Tests/DecodexAppTests/HoverHintTests.swift
  - id: openwiki-source-2fc387fccf8dd1285437f49e
    resource: repo://apps/decodex-gpui/src/agent_activity.rs
  - id: openwiki-source-dac0b6b29f475a07a99ba3a4
    resource: repo://apps/decodex-gpui/src/agent_async_questions.rs
  - id: openwiki-source-b243cfb8ae3053f8047fdab0
    resource: repo://apps/decodex-gpui/src/agent_capabilities.rs
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
  - id: openwiki-source-4f748464984e05f0240896f2
    resource: repo://apps/decodex-gpui/src/agent_command_palette_tests.rs
  - id: openwiki-source-36b7f34eeb55f2eec55e903d
    resource: repo://apps/decodex-gpui/src/agent_command_palette.rs
  - id: openwiki-source-48758ba7d154c82885281d2c
    resource: repo://apps/decodex-gpui/src/agent_composer.rs
  - id: openwiki-source-a65c3fa1ed74697b3bd42810
    resource: repo://apps/decodex-gpui/src/agent_conversation_metric.rs
  - id: openwiki-source-5080be62f09cdc91d9e2f37b
    resource: repo://apps/decodex-gpui/src/agent_handoff_view.rs
  - id: openwiki-source-1ee22a183019eba898707d3c
    resource: repo://apps/decodex-gpui/src/agent_history_preview_tests.rs
  - id: openwiki-source-079a15445a0295f222af8069
    resource: repo://apps/decodex-gpui/src/agent_model_settings.rs
  - id: openwiki-source-ae2ce5cda718ede63f50be9b
    resource: repo://apps/decodex-gpui/src/agent_native_composer.rs
  - id: openwiki-source-e59b6fade5576eca54cd30f7
    resource: repo://apps/decodex-gpui/src/agent_native_menu.rs
  - id: openwiki-source-3664c9b8790465805ef686fd
    resource: repo://apps/decodex-gpui/src/agent_navigation_actions.rs
  - id: openwiki-source-782cfab091c6439c4ff4dee2
    resource: repo://apps/decodex-gpui/src/agent_navigation_icons.rs
  - id: openwiki-source-2ca09fa226c111bf23378654
    resource: repo://apps/decodex-gpui/src/agent_navigation_projects.rs
  - id: openwiki-source-c2592b96974e70b72f93f72b
    resource: repo://apps/decodex-gpui/src/agent_navigation_sidebar_tests.rs
  - id: openwiki-source-541c96314da57ad5afa1835e
    resource: repo://apps/decodex-gpui/src/agent_navigation_sidebar.rs
  - id: openwiki-source-15d320ea458ddf705d950ba9
    resource: repo://apps/decodex-gpui/src/agent_response_metrics.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-691432bb61082c358b6f9c24
    resource: repo://apps/decodex-gpui/src/agent_timeline.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-7131f576ee57d12a847fe177
    resource: repo://apps/decodex-gpui/src/agent_work_progress.rs
  - id: openwiki-source-c0e58bc836ac05c982449723
    resource: repo://apps/decodex-gpui/src/agent_workbench.rs
  - id: openwiki-source-e269da8230b6d51390181459
    resource: repo://apps/decodex-gpui/src/agent_workspace_size.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-686aae4ae35c7f99c92e3d9b
    resource: repo://apps/decodex-gpui/src/bin/probes/native_island.rs
  - id: openwiki-source-328b0f351dacca05a8787dd5
    resource: repo://apps/decodex-gpui/src/bin/probes/native_popup_system.rs
  - id: openwiki-source-b485e07e98ac03dcff5db641
    resource: repo://apps/decodex-gpui/src/bin/probes/native_workspace.rs
  - id: openwiki-source-1bc5c8a6672bb09ef7b2d30f
    resource: repo://apps/decodex-gpui/src/bin/workbench_visual_capture.rs
  - id: openwiki-source-7b89f5bc6ecd604ca4e97dc2
    resource: repo://apps/decodex-gpui/src/composer_text.rs
  - id: openwiki-source-303e0ddb3b6a519e014a6fd2
    resource: repo://apps/decodex-gpui/src/conversation_divider.rs
  - id: openwiki-source-8baa689fd9c859a6d9a120d5
    resource: repo://apps/decodex-gpui/src/frame_trace.rs
  - id: openwiki-source-37f9a508a2aebf9382d3b23f
    resource: repo://apps/decodex-gpui/src/main.rs
  - id: openwiki-source-fca3688e96d92c09b7ba015f
    resource: repo://apps/decodex-gpui/src/native_material_panel.rs
  - id: openwiki-source-1291f5243fa6c9cb52149bda
    resource: repo://apps/decodex-gpui/src/shell.rs
  - id: openwiki-source-592957b8edd8a689f738c189
    resource: repo://apps/decodex-gpui/src/ui_frame.rs
  - id: openwiki-source-adab7a75320a305721fdd6d2
    resource: repo://apps/decodex-gpui/src/ui_hover_intent.rs
  - id: openwiki-source-39b5e0e9a125062f8acc2126
    resource: repo://apps/decodex-gpui/src/ui_layout.rs
  - id: openwiki-source-ccd97a4d4ef8dfe9b724e7ce
    resource: repo://apps/decodex-gpui/src/ui_live_activity.rs
  - id: openwiki-source-2316e3cffd10487b54cd63f4
    resource: repo://apps/decodex-gpui/src/ui_material.rs
  - id: openwiki-source-afbf2d30c0979a844373d8e5
    resource: repo://apps/decodex-gpui/src/ui_motion.rs
  - id: openwiki-source-79f9e926ceb3e6b2643c73a5
    resource: repo://apps/decodex-gpui/src/ui_native_popover.rs
  - id: openwiki-source-9e7fc8b7290695ff7afa8373
    resource: repo://apps/decodex-gpui/src/ui_overflow_label.rs
  - id: openwiki-source-8f7f43de2b98b69bb43506d7
    resource: repo://apps/decodex-gpui/src/ui_performance_readout.rs
  - id: openwiki-source-8c633c09481985d80c8f42f2
    resource: repo://apps/decodex-gpui/src/ui_performance_tests.rs
  - id: openwiki-source-6d7ff71d731da7268890d765
    resource: repo://apps/decodex-gpui/src/ui_performance.rs
  - id: openwiki-source-546812ff3637690f4a184b89
    resource: repo://apps/decodex-gpui/src/ui_preferences.rs
  - id: openwiki-source-41bad78d71f1e6614f8d340d
    resource: repo://apps/decodex-gpui/src/ui_rail_label.rs
  - id: openwiki-source-570564020f98df7a4d6d0b61
    resource: repo://apps/decodex-gpui/src/ui_theme.rs
  - id: openwiki-source-c3fab9156f998a4c38613276
    resource: repo://apps/decodex-gpui/src/ui_time.rs
  - id: openwiki-source-b17dbfde5c614f0bffe7770a
    resource: repo://apps/decodex-gpui/src/ui_tooltip.rs
  - id: openwiki-source-fa218a67bba67197ecf88d7b
    resource: repo://apps/decodex-gpui/src/ui_viewport_clip.rs
  - id: openwiki-source-8cb8ccb33352e11a3da74500
    resource: repo://apps/decodex-gpui/src/ui_work_metrics.rs
  - id: openwiki-source-343d35c489d39a7a672412a2
    resource: repo://apps/decodex-gpui/src/ui_working.rs
  - id: openwiki-source-5a4dfc4ce84de6c05c56d3fb
    resource: repo://apps/decodex-gpui/src/voice_history_block.rs
  - id: openwiki-source-8f413b540123b1fe73628c7d
    resource: repo://apps/decodex-gpui/src/window_chrome.rs
  - id: openwiki-source-35beef376e748f87821e9639
    resource: repo://apps/decodex-gpui/src/workspace_symbols.rs
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-a490ea3fbcddb9c007caf196
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_context_tests.rs
  - id: openwiki-source-8fe6f1557e5d45d88b34bd0a
    resource: repo://scripts/macos/generate_workspace_symbols.swift
generated: { by: "codex", at: "2026-11-20T07:13:28.942Z" }
---

# Interface geometry and motion

Use shared geometry and presentation components when you add a desktop surface. `ui_theme` owns spacing, type and color tokens. `ui_live_activity` owns activity-island size tiers and motion. A feature owns its data and actions. It does not need another animation system.

Use `MaterialSurface` for persistent floating controls such as **Jump to latest message**. These controls follow the **Overlay material** and **Overlay glass style** preferences. Apply `MaterialSurface::opacity` to fade the native material, shadow and foreground together. When the owner controls opacity, do not add a second native arrival animation. Retain the control's geometry and actions while its surface changes. Hide its attached window when the owning control is no longer present.

## Activity islands

Work and Debug use the same geometry contract. Work uses the waveform symbol. Debug uses the performance gauge. Each text header reserves a 14-point symbol and a 6-point gap in its measured width. The symbols use the current Decodex accent color.

| Property | Rule |
| --- | --- |
| Collapsed height | 28 points, from the shared control-group height. |
| Minimum text width | Round measured content up to the 8-point grid, with a 72-point minimum. |
| Symbol-only width | When the row cannot fit the measured minimum text widths, give each symbol the same space, up to the 28-point capsule height. |
| Compact width | Measure the header content and round up to the shared 8-point grid. |
| Expanded width | Fit the header and detail content on the shared 8-point grid; do not impose an oversized preset. |
| Space between islands | 6 points. |
| Corner radius | 14 points when collapsed, increasing to 20 points when expanded. |
| Transition | 240 ms, with a cubic ease-out and no overshoot. |
| Detail appearance | Fade details during the final 45 percent of the height transition. |

Only one activity expands at a time. A pair uses two equal fixed slots with a 6-point gap, so its inner anchors stay in place when selection or metric digits change. Each activity grows outward within its side of the available space. Do not translate the pair to make room for expansion. Measure each header and detail grid from visible content; an idle Work header does not reserve a hidden generation-rate field. A single activity remains centered. If a full detail grid cannot fit, use the compact or minimum tier without squeezing its columns. Retain all meaningful metrics and keep the sparkline legible.

`Morph` uses one clock for width and height. A reversal starts at the current visible size. Reduced motion selects the target size directly. Round visible dimensions to physical pixels using the current display scale.

Keep the native drawable at the complete content size and at the window origin during a transition. `MaterialSurface::clip` puts the foreground beside the material and applies a rounded foreground mask. Update the material contour and foreground mask in the same Core Animation transaction with implicit actions disabled. Reuse unchanged clipping geometry; `Morph` owns the transition. Do not resize the Metal drawable on each animation frame. The input region follows the visible contour, so the clear area outside an island does not block controls below it.

Lay out detail text and metric columns at the complete content width. Clip that layout during opening and closing. Values retain one view identity and move along a defined path. Labels follow their corresponding values and fade in only when the complete field fits with its required separation. A narrow intermediate header must not paint secondary text over its main value. Do not reveal a label by repeatedly changing its text width or paint two copies of the same value at different positions.

## Responsive workspace

The main window accepts sizes down to 480 by 360 points. Reserve a 360-point reading width and the 40-point navigation rail before allocating persistent side panels. Always allocate the left navigation before the right panel, regardless of opening order. A right panel that cannot fit floats above the reading plane. If the requested full navigation cannot fit, it returns to the rail and keeps its pinned preference. Resizing to a wider window restores the requested full navigation. The left hover preview clips the overlap with the right panel. Collaboration keeps its size and position.

Use the same 8-point outer gutter and 16-point corner radius for both side panels. Their requested width is between 160 and 480 points, constrained by the window. Keep the complete panel content layout behind the animated clipping edge. Both panels use the original `ui_theme::workspace_surface` and its workspace field color in the main GPUI window, including floating panels and the compact navigation rail. Do not create a separate native Liquid Glass surface for a sidebar. Both content roots use the same system font, 12.5-point body size and 19-point line height. The existing clipping transition owns their motion. Escape first dismisses a temporary panel before it can interrupt a reply. A click in the uncovered reading plane also dismisses it. Preserve the docked panel, Graph state and conversation draft.

Sample the left and right layout slots and visible panel widths together once per workspace frame. The sidebar, conversation header, reading-plane clip and native composer mask all use that same `PanelFrame`. Reading geometry later in the frame must not advance an independent animation clock. Keep the currently painted mask until the next frame; opening, closing and reversal reveal content at the moving edge rather than switching complete controls on or off.

Docked and floating placement changes allocation and dismissal, not the material or typography. Keep the placement state during closing and a reversal. Do not infer floating placement from a difference between the animated width and the target layout slot: closing releases that slot before the visible panel has disappeared.

Keep the same workspace surface while a panel changes placement or width. Do not blend between separate workspace and floating materials. The sidebar derives its reveal geometry from the main window and uses the shared viewport mask. `ui_layout::pane` can reuse an unchanged presentation in that window. State changes, activity animation and hover updates invalidate the presentation normally. Active accessibility uses a live semantic tree. The sidebar itself owns pointer entry and departure in both expanded and compact states. Entry starts the shared dwell even when the pointer stops without another mouse move. The input bar, activity islands and menus retain their separate native material behavior.

An unset Overlay glass style inherits Window glass style, including later changes. An explicit Regular or Clear override remains independent. The inherited value does not create a saved override.

The compact navigation rail and its temporary hover preview stay full height, including during automatic collapse and restoration. Keep the Archived footer at the bottom; do not lift it above the composer. Only an explicitly pinned full navigation panel that must float, and the right floating panel, end above the complete composer area, including Work in progress and conversation feedback. Floating panels cover the reading plane without changing its width. Clip covered Graph, history and pointer regions without reflow. Use `ui_motion::revealed_value` for the lower edge. A panel that opens into a known floating placement starts at its calculated height, without a separate lower-edge transition. Animate later height changes while the panel is already visible. Closing and reversal retain continuity until the panel reaches its closed state. Graph expansion retains navigation and collaboration panels. Explicit conversation focus mode can still hide the side panels.

The native composer uses one selected system material. Do not add a second GPUI background fill or a window backdrop below it. Text and controls use the current theme. Measure the native content at the latest parent anchor width before painting so the content, glass contour and input region stay aligned during resize. A full-height navigation preview can cover part of the composer. Mask the covered part of the complete native composite and its pointer region; keep the original material contour, foreground size and draft. Apply that native mask during parent prepaint, before the corresponding frame is painted. Close the temporary preview when the user focuses the composer. Retain the editor and its focus ownership.

When Graph is hidden, an existing conversation with unfinished scoped work can show **Work in progress** above the composer. Use the shared 28-point control height, 14-point Work symbol and intrinsic disclosure motion. Include unfinished tasks and observed native agents from the current conversation's work subtree. Do not insert example work. Limit the expanded list to one quarter of the window height, between two and six control rows, and scroll extra rows. A row opens the existing conversation. Hide the strip in a new conversation or when there is no matching work.

## Reading rhythm

| Relationship | Space |
| --- | --- |
| Separate conversation messages | 20 points. |
| Completed process header and its answer | 6 points. |
| Reply content and working indicator | 4 points. |
| Working indicator and composer | 20 points in total. |
| Working indicator | 24-point status row. |

The composer contributes 8 points to the lower reading gap. Count that contribution when you position a status row. When a reply finishes, crossfade from the working indicator to the response metrics on the same 24-point line. Show delivery records below that line. Keep the ordinary gap before the composer. Folding process details must preserve the final answer and its navigation identity.

Use `conversation_divider` for both process disclosures and complete Live sections. Its Process variant keeps the shared 11-point normal muted metric caption in a 20-point row. Its Section variant uses the shared 12.5-point body size, semibold primary ink and a 24-point row. Both variants share the leading chevron, full-height hit target and short fading hairline. A Live section uses the shared 6-point process-content gap below its divider for either the folded source preview or the first expanded speaker turn. Expansion replaces the preview with the transcript. Speaker turns keep the 20-point reading gap. The shared 4-point metadata gap separates the last visible text from the response footer.

## Conversation start

New project Agent and an unstarted personal Assistant use a centered 20-point start heading above the composer when history is confirmed empty. The project Agent shows **What would you like to do?** with its project chooser. The personal Assistant shows **What’s next?** and **A new topic, with the same Assistant.** It has no project chooser. The empty composer keeps its quote placeholder.

The start heading gives way to saved messages, a sending preview, or a retained failed or unconfirmed input. A running response, a question, or a misalignment also keeps the conversation presentation. Missing or unavailable history uses the existing loading or retry presentation. Do not use a passive first-message waiting label for an empty conversation.

## Navigation and attention

Use three independently collapsible categories: **Pin**, **Assistant** and **Projects**. Each project has its own disclosure. Several projects can remain open at the same time. Categories, project headers and conversations share one scrolling directory. Keep **Search and commands** above that directory and **Archived** at the bottom. Every label shares the same text column after one 28-point leading slot. Use weight and group spacing to express ownership without tree guides or additional indentation.

Pin uses the warm gold theme slot, Assistant uses blue and Projects uses teal. Projects uses a filled folder; each expanded project uses a neutral outline folder. Project names and folder symbols use neutral ink. Project IDs do not select colors. Use the shared native SF Symbol artwork through `workspace_symbols`. The macOS generator supplies the symbol assets. In the compact sidebar, project and conversation rows show the first complete grapheme of their name in the centered leading slot. Conversation initials use the shared state ink. Chinese characters, combined accents and emoji remain intact. Category and utility controls keep their icons. Expanded titles remain one normal-weight text run.

In the expanded sidebar, running conversations show three dots with a repeating opacity pulse. Reduced motion keeps the dots steady. Requests for attention use an amber ring and failures use the same ring in red. Unread results use a filled blue dot. Rings and the result dot use the same 6-point diameter. Opening the conversation and loading its available history clears that result's unread mark. Quiet conversations and completed conversations with no unread result leave the slot empty. The title column stays fixed through status changes.

| Property | Value |
| --- | --- |
| Utility, category, project and conversation row | 28 points. |
| Conversation and utility text | 12.5 points, normal weight. Conversation titles use body color. |
| Project title | 12.5 points, normal weight, body color. |
| Category title and Archived | 12.5 points, medium weight, primary color. |
| Relative update time | Shared 10.5-point metric text, at the far right. |
| Row corner radius | 5 points. |
| Gap between adjacent conversation rows | 0 points. |
| Gap after an open project group | 8 points. |
| Gap after a closed project | 0 points. |
| Divider slot | 12 points, with a quiet fading line. |
| Collapsed rail | 40 points, including two 6-point insets. |
| Leading symbol slot | 28 points within the 28-point row. |
| Category and project symbols | 16 points. |
| Conversation mark frame and creation symbol | 14 points. |
| Sorting symbol | 16 points, native up/down arrows. |
| Disclosure symbols | 12 points. |
| Small action target | 24 points. |

Assistant contains its personal conversations. Pin contains shortcuts to the same conversations; pinning does not remove source membership. A project disclosure opens or closes its own conversations without closing other projects. Category disclosure preserves the open-project choices. Both operations preserve the selected conversation, its draft and its loaded transcript. Use the shared intrinsic disclosure and chevron motion. Project groups have no separate cards, background fields or connector lines.

Keep sorting and creation beside their category or project title. They appear on pointer hover or keyboard focus and use one shared width and opacity transition. Hidden controls release their width and leave no active pointer target. The disclosure stays at the same trailing position. An open options menu keeps its owner controls visible. The Projects category retains **New project**. Assistant and project headers expose their respective new-conversation actions through the same controls. Empty expanded groups show a short explanation in one ordinary 28-point row rather than a fabricated conversation. The compact rail omits these explanation rows and project group spacing, so empty groups leave no blank gaps. Empty unsent editors remain outside the conversation list. Archived uses the category label style and shared disclosure, with a divider above its fixed bottom row.

Project headers expose a **Delete project** icon beside the new-conversation action on hover or keyboard focus. The first click turns the delete icon red and applies the shared `ConfirmationNudge` inside its fixed 24-point target. A second click removes the project. Leaving the row, pressing Escape or clicking outside cancels. The confirmation hint states that chats and files are kept. Project removal does not replace the current conversation or discard its draft. No separate menu or confirmation panel is added.

The directory and archive browser have separate retained scroll handles. Category and project expansion choices belong to the current window. The default closes Pin and opens Assistant and Projects. Opening a saved conversation reveals its required category and project. **Search and commands** also opens the required group and scrolls to the target without closing other projects. Project creation and conversation actions retain their existing service and draft ownership.

Keep Search and commands in the same leading slot when the sidebar contracts. It opens the global command window, also available with Command-K. The compact rail retains category and utility symbols, plus the first complete grapheme for each project and conversation. A pointer dwell of 100 ms temporarily expands it as a full-height floating surface, including after automatic collapse. Uncovered Collaboration content stays visible and accepts input while the hover preview is open. Leaving starts a 220 ms close delay; returning cancels that close. Reuse the shared width transition and keep the leading symbols fixed. A category or project disclosure during a hover does not pin the sidebar. The explicit sidebar toggle controls persistent visibility and clears temporary hover state. Relative times and trailing actions do not add width or height to the compact rail.

The sidebar's outer width uses the shared interruptible 200 ms cubic ease-out. Its content retains the complete expanded canvas during opening, closing and a reversal; the outer panel clips that canvas. Keep titles and trailing controls at their final positions during the transition. Do not squeeze text into intermediate widths or remove labels as soon as the target changes. The settled compact rail exposes category symbols and project or conversation initials. Its 28-point symbol column is centered between two 6-point insets in the 40-point rail, so labels start at the clipping edge and no partial glyph remains visible.

Keep the complete sidebar text at its final position and color. `ui_motion::Viewport` measures the actual pane bounds and clips the fixed content at the visible edge, including its shared inner padding. The edge reveals or covers the existing text. Do not give labels an opacity transition, character counter, normalized reveal fraction or separate clock. Category, project and conversation leading slots remain fixed. A reversal changes only the current clipping geometry. Reduced motion selects the target geometry directly.

Use `ui_motion::HoverIntent` for surfaces revealed by pointer intent. The shared tokens set entry dwell to 100 ms and departure grace to 220 ms. Brief crossings cancel entry. Movement inside the same target does not restart its timer. Reentry cancels closing, and an exit from an older target cannot cancel a newer target. Explicit clicks, keyboard actions and dismissal cancel pending pointer work and retain their immediate behavior. The sidebar, Work and Debug islands, response-statistics popover and history previews use this owner. A history tick group and its card form one hover target: scrubbing changes the message immediately while retaining the surface and its arrival state. Ordinary hover ink, contextual row actions and native tooltip lifetime retain their own presentation roles.

Keep the Timeline rail centered in the minimum 24-point gap between the message edge and the trailing workspace edge. Its center stays 12 points from the workspace edge as the window widens. The 16-point hit target extends into the workspace gutter, and the reading-plane mask clips any part that a floating panel covers.

Group previews use `Popover::hover_preview` with the full trigger group as their anchor. Keep the 320-point Timeline preview width fixed. The first appearance uses its final measured height; subsequent content changes use the shared 200 ms interruptible `Tween`. Reversal continues from the current height. Keep the trigger anchor stable and use the common hover placement contract below for available space and edge clearance. Native and GPUI hosts use the same placement and height policy. Keep text at its measured size behind the changing frame, and resolve the target directly for reduced motion. Ordinary menus retain their existing placement and fixed-size arrival.

Use `ui_time` as the single formatter for relative age, elapsed duration, complete local timestamps and the current clock. Sidebar ages and Timeline ages use the same minute, hour, day, week, month and year units. They share `ui_theme::metric_text` and `metric_ink`: normal 10.5-point caption text, a 14-point line height, tabular digits and muted theme ink at 85% opacity. History previews show only relative age, using the same calculation as navigation. Keep both excerpts left-aligned in one full-width text column, using the same normal theme body color, font size, weight and line height for input and reply. Place the shared 0.5-point menu-separator hairline inside the existing 10-point gap between them. Do not increase the gap or card height for the divider, and omit it when the reply is missing. Input has at most two lines; the reply has at most three. Do not add message-direction marks, message bubbles or speaker headings. Use the shared compact-body tokens at 12 points with an 18-point line height and normal weight. Separate the final excerpt and metrics by 8 points with `ui_theme::METRIC_GROUP_GAP`, without a second divider. Metrics use the shared 10.5-point caption and 14-point line height. Inherit the interface font and its language fallback; keep tabular numeric features in the metadata. Font changes must preserve readable line bounds and the same five-column allocation.

Use `agent_conversation_metric.rs` for compact conversation metric symbols, value formats, caption type, symbol-value gaps and base ink. Native response footers and Timeline previews must render its shared `Metric` elements. Duration uses the existing response clock, input/output use fixed vector up/down symbols, age uses a history clock, and explicit status uses a neutral circle with text. Steps use the footprints symbol. The compact value is a bare number, including `0`; unknown uses `—` and active work uses `…`. The main process header and accessibility name retain the full step label. Ordinary recorded counts add no hint. Unknown counts, running processes and terminal failures retain their explanations. Do not reserve an independent width for Steps. Count the records in the folded process and exclude its last displayed final answer. Icons and values inherit the same color. The base ink is muted theme text at 85% opacity; interactive response rows apply hover ink to the complete row. Conversation metrics and `ui_work_metrics::time_label` delegate duration formatting to `ui_time`. Whole seconds omit the unnecessary `.0`; durations of at least one day show days and remaining hours, such as `5d 3h`. Recorded zero is `0s`; missing duration is `—`. Keep symbol size and shape independent of the selected font. Keep preview metadata in one caption-height row with five equal-width columns for age, duration, steps, input and output. Center every symbol-value group within its own column. Keep all columns present and use an em dash for each unknown value. Value length and availability must not change column allocation. Do not nest metric groups with different spacing rules or show model-call counts. Preserve the difference between unknown native response values and recorded zero, and use the exact turn's recorded usage. Do not add a divider or second row. Do not substitute thread totals or billing.

Ordinary reply and Live footers use the same Time, Input, Output and Spend components and details popover. Spend uses a fixed coin symbol and the existing USD formatter. An approximation mark identifies estimates, and a plus sign identifies incomplete coverage. Missing data keeps its symbol and an em dash; measured or calculated zero stays explicit. Details explain missing receipts, pricing inputs and estimate scope. Copy and Side chat remain separate actions in the shared footer. These four response fields do not change the five-column history-preview layout.

A committed pointer intent or explicit surface toggle starts a bounded 400 ms animation window. A visible inactive App can therefore finish the transition without taking foreground focus. This does not change the Full background rendering preference. When that window expires, the ordinary background policy applies again. System reduced motion and VoiceOver still select the target geometry directly. Continuous activity does not renew this window; a new user operation does.

Long titles retain their full text. Hover or keyboard focus starts overflow motion; leaving returns the text smoothly. Each section can enable Always scroll titles. Use shared disclosure, chevron, popover, control-opacity and confirmation-nudge motion. Respect reduced motion and do not animate the complete transcript for a sidebar hover.

Conversation rows reveal **Pin** or **Unpin**, **Archive** and **Delete** as three inline icons on hover or keyboard focus. Archived rows reveal only **Restore** and **Delete**. The relative time fades while these actions appear. The first Delete click turns its icon red and applies the shared confirmation nudge; the next click confirms deletion without another prompt. The icon keeps the same 24-point hit target. Leaving the row, pressing Escape or clicking outside cancels. Icon hints use short action names and explain confirmation or unavailable actions when needed. Deletion keeps learned Assistant Memory. Archived rows reuse the native restore and delete paths. Anchor the owner, time and active status tooltip to the complete row through the common hover placement contract so it stays clear of the row and its actions.

Use restrained warm emphasis for requests that need the user. Derive this state from a pending native request, an unanswered native question, or an explicit user decision. Do not classify attention from a displayed label. Returned agent work and ordinary collaboration remain distinct from requests for a user decision. Window controls use fixed-size icons, with names in tooltips and accessibility labels. Collaboration retains a small pending-count badge when its panel is closed; it adds no visible toolbar label.

## Hover content and surfaces

A hover hint must add useful information. Keep the action name for an icon-only control, a shortcut, a hidden value, a failure reason or the inputs behind an estimate. Remove a hint when it repeats a visible label or explanation. Use short action names and explicit value labels. Keep accessibility names when a visible hint is removed. Expanded navigation utilities, ordinary recorded step counts, visible answer options and Live Mute, Unmute, End and Cancel controls do not need duplicate explanations. A project hint can show its hidden directory. A conversation hint includes the owner, update time and active status when available. The compact hint also includes its title.

GPUI callers attach text with `ui_motion::HoverHint::hover_hint` or use `tooltip_at` for anchored metadata. The common owner supplies the complete surface. It uses the interface font at 12 points, an 18-point line height, 8-point horizontal and 6-point vertical insets, a 360-point maximum width, a 10-point corner radius and the shared 0.5-point outline. Feature callers supply content, not another background or shadow. Text can wrap. Fit a text hint to the smallest width that preserves its line count at the maximum width. This removes unused trailing space without adding lines or changing the text. Cost explanations use short labelled lines for precise amount, observed responses and the price-check date.

Anchor each hint and detail preview to the complete trigger bounds. Movement inside that control must not move the surface. Keep an 8-point gap from the trigger and 8-point clearance from the available area. Try below, then above, then the right and left sides. Fit each candidate to the area and use the first that clears the trigger and its gap. Native windows use the screen’s visible bounds; a hint from the input bar can therefore extend above that small native window. The GPUI fallback uses its window bounds. Features must use this shared calculation instead of cursor offsets or separate placement code. The SwiftUI adapter applies the same rule in screen coordinates.

Passive hints follow **Overlay material** and **Overlay glass style**, as do rich previews. The native GPUI host measures the hint in the framework tooltip slot and renders its foreground once in a passive material window. It retains one reusable hint host per parent, separate from interactive menus and attached controls. A hint does not take mouse input or keyboard focus. GPUI owns its 500 ms dwell and tooltip lifetime; leaving or removing the control releases it. Opening an explicit popup hides a previous hint. The shared native departure observer clears GPUI’s retained pointer position after `MouseExited`. Both tooltip anchors and interactive popup hosts use it, so a child window cannot retain a stale hover after the pointer leaves. The GPUI fallback uses the same content geometry and outline.

The SwiftUI menu-bar adapter uses `hoverHint` and one reusable `HoverHint` panel with the same material preference keys and hint geometry. It also waits 500 ms and keeps pointer and keyboard input with its owner. A key menu-bar panel can show hints without activating the application. Departure, owner removal, focus loss, closing, scrolling and input cancel stale hints. Account quota values and already-visible Used or Using states do not add duplicate popups; hidden errors and reset-card confirmation instructions remain available.

Interactive history and metrics previews retain their existing `Popover` and `HoverIntent` owners, content widths, entry dwell and departure grace. The pointer can enter these previews to inspect their contents. Activity-island and navigation expansion retain the shared sizing and motion contracts. Ordinary hover ink and inline control feedback stay with the control.

## Recovery feedback

Name the cause that the application can establish. A missing selected working folder shows **Working folder is unavailable. Restore it to continue.** Other generic admission failures show **Reconnecting to Codex…** and retain the current technical detail. Account readiness failures keep their specific Accounts guidance. A local folder failure must not imply a network outage.

Keep provider stream retries in native Codex. A retrying stream error does not authorize another user input or a new turn. The isolated native recovery check closes one local SSE response before completion, observes the retry while the original turn stays running, then verifies completion on the same thread and turn. This checks a controlled stream interruption; it does not qualify every physical network failure.

## Theme and controls

On macOS, AppKit owns the window-button and window-sharing geometry. The main window uses a unified compact native toolbar. GPUI follows the measured native control-row center for the leading controls, activity islands and trailing controls. Keep `traffic_light_position` unset. Do not move native window buttons or their parent containers to fit a custom header. AppKit retains blank titlebar dragging.

Control groups use the shared 28-point height, 24-point control targets, 4-point inner spacing and gap, 8-point outer inset and capsule radius, and 5-point small-control radius. Dense navigation retains its 6-point inner inset. Use these role tokens instead of feature-specific values. The titlebar groups share the native control-row centerline. Draw one leading surface behind the AppKit window controls and app navigation buttons. Extend this surface 4 points toward the window edge and add the same amount to its leading padding. The trailing group uses the 8-point window inset and the same 4-point gap on both sides of the divider between panel controls and app utilities. Keep the control targets and gaps at the minimum window width. Attached material surfaces retain their allocated anchor, including the clipping canvas; the native host must not add a menu edge inset. Independent floating menus use the shared 8-point edge clearance.

Use the current theme tokens for foreground, muted labels, emphasis and status colors. The Ultrafast double bolt uses the theme accent. Its silhouette is shared across the composer; its color is not copied from another application.

Use a quieter color for keys and supporting labels, and the normal foreground for values. Keep settings explanations in complete sentences. Put independent actions in a separate row when their purpose differs: account routing belongs to **New conversations**, while email visibility belongs to the account-list controls.

The composer model menu opens only when the current catalog contains models. A known model stays visible during catalog refresh or failure. Without a known model, the closed control shows `-`. Startup resolves the model automatically; it does not present a selection prompt. Keep loading and empty states outside the model popover. A suggestion is an editable placeholder, wraps within the normal editor and never sends itself. A quote belongs only to a new conversation without history.

## Extend and verify

`AGENTS.md` requires every activity island to use this shared contract. Reuse `Sizing::content`, `arrange`, `Morph`, `MaterialSurface::clip` and the spacing constants. A new variant must update the shared contract and its focused checks. Keep feature-specific content outside the geometry owner. Use the same components in isolated fixtures and the installed application.

The layout tests cover size allocation, fixed canvas bounds, a transition reversal, reduced motion, status spacing and the process-to-answer gap. The native island probe exercises 20 Work and Debug transitions, including four reversals before an opening completes, without a daemon or model request. It checks that the foreground remains at the window origin and that native bounds match the GPUI viewport. The visual-capture binary also accepts `DECODEX_VISUAL_NATIVE_WORKSPACE` with an explicit `DECODEX_FRAME_TRACE` path and isolated `DECODEX_VISUAL_DRAFT_ROOT`. This display-backed workspace probe exercises 12 left, right and combined transitions, including interrupted closing, with the existing Graph fixture. It uses process-local appearance settings. The `DECODEX_VISUAL_NATIVE_COMPOSER` variant resizes a real AppKit composer with the workspace and toggles the right panel. Its trace records the native foreground, viewport and Metal drawable dimensions without a live service. Frame traces count content rebuilds and record CPU draw and submission timing; they do not measure GPU completion or screen scanout. Use the installed app for final mouse, clipping and visual checks.

See [conversation presentation](conversation-presentation.md) and [models and settings](../workflows/models-and-settings.md) for the related product flows.
