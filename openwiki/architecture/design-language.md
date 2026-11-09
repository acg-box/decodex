---
type: Architecture
title: Interface geometry and motion
description: Reusable layout, activity-island motion, reading spacing and theme rules for the native Decodex desktop.
tags: [desktop, design, motion, layout]
verified:
  - by: openwiki/0.9.0
    at: 2026-11-09T06:37:11.127Z
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
  - id: openwiki-source-48758ba7d154c82885281d2c
    resource: repo://apps/decodex-gpui/src/agent_composer.rs
  - id: openwiki-source-079a15445a0295f222af8069
    resource: repo://apps/decodex-gpui/src/agent_model_settings.rs
  - id: openwiki-source-ae2ce5cda718ede63f50be9b
    resource: repo://apps/decodex-gpui/src/agent_native_composer.rs
  - id: openwiki-source-3664c9b8790465805ef686fd
    resource: repo://apps/decodex-gpui/src/agent_navigation_actions.rs
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
  - id: openwiki-source-b485e07e98ac03dcff5db641
    resource: repo://apps/decodex-gpui/src/bin/probes/native_workspace.rs
  - id: openwiki-source-1bc5c8a6672bb09ef7b2d30f
    resource: repo://apps/decodex-gpui/src/bin/workbench_visual_capture.rs
  - id: openwiki-source-7b89f5bc6ecd604ca4e97dc2
    resource: repo://apps/decodex-gpui/src/composer_text.rs
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
  - id: openwiki-source-570564020f98df7a4d6d0b61
    resource: repo://apps/decodex-gpui/src/ui_theme.rs
  - id: openwiki-source-fa218a67bba67197ecf88d7b
    resource: repo://apps/decodex-gpui/src/ui_viewport_clip.rs
  - id: openwiki-source-343d35c489d39a7a672412a2
    resource: repo://apps/decodex-gpui/src/ui_working.rs
  - id: openwiki-source-8f413b540123b1fe73628c7d
    resource: repo://apps/decodex-gpui/src/window_chrome.rs
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-a490ea3fbcddb9c007caf196
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_context_tests.rs
generated: { by: "codex", at: "2026-11-09T06:37:11.127Z" }
---

# Interface geometry and motion

Use shared geometry and presentation components when you add a desktop surface. `ui_theme` owns spacing, type and color tokens. `ui_live_activity` owns activity-island size tiers and motion. A feature owns its data and actions. It does not need another animation system.

## Activity islands

Work and Debug use the same geometry contract. Work uses the waveform symbol; Debug uses the performance gauge. Each header reserves a 14-point symbol and a 6-point gap in its measured width. The symbols use the current Decodex accent color.

| Property | Rule |
| --- | --- |
| Collapsed height | 28 points, from the shared control-group height. |
| Minimum width | At least 72 points; round measured content up to an 8-point grid. |
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

The main window accepts sizes down to 480 by 360 points. Reserve a 360-point reading width and the 40-point navigation rail before allocating persistent side panels. Always allocate the left navigation before the right panel, regardless of opening order. A right panel that cannot fit floats above the reading plane. If the requested full navigation cannot fit, it returns to the rail and keeps its pinned preference. Resizing to a wider window restores the requested full navigation. Opening the left preview temporarily hides a right floating panel and restores it when the preview closes.

Use the same 8-point outer gutter and 16-point corner radius for both side panels. Their requested width is between 160 and 480 points, constrained by the window. Keep the complete panel content layout behind the animated clipping edge. Both panels use the original `ui_theme::workspace_surface` and its workspace field color in the main GPUI window, including floating panels and the compact navigation rail. Do not create a separate native Liquid Glass surface for a sidebar. Both content roots use the same system font, 12.5-point body size and 19-point line height. The existing clipping transition owns their motion. Escape first dismisses a temporary panel before it can interrupt a reply. A click in the uncovered reading plane also dismisses it. Preserve the docked panel, Graph state and conversation draft.

Sample the left and right layout slots and visible panel widths together once per workspace frame. The sidebar, conversation header, reading-plane clip and native composer mask all use that same `PanelFrame`. Reading geometry later in the frame must not advance an independent animation clock. Keep the currently painted mask until the next frame; opening, closing and reversal reveal content at the moving edge rather than switching complete controls on or off.

Docked and floating placement changes allocation and dismissal, not the material or typography. Keep the placement state during closing and a reversal. Do not infer floating placement from a difference between the animated width and the target layout slot: closing releases that slot before the visible panel has disappeared.

Keep the same workspace surface while a panel changes placement or width. Do not blend between separate workspace and floating materials. The sidebar derives its reveal geometry from the main window and uses the shared viewport mask. `ui_layout::pane` can reuse an unchanged presentation in that window. State changes, activity animation and hover updates invalidate the presentation normally. Active accessibility uses a live semantic tree. The sidebar itself owns pointer departure in both expanded and compact states. The input bar, activity islands and menus retain their separate native material behavior.

An unset Overlay glass style inherits Window glass style, including later changes. An explicit Regular or Clear override remains independent. The inherited value does not create a saved override.

The compact navigation rail and its temporary hover preview stay full height, including during automatic collapse and restoration. Keep the Archived footer at the bottom; do not lift it above the composer. Only an explicitly pinned full navigation panel that must float, and the right floating panel, end above the complete composer area, including Work in progress and conversation feedback. Floating panels cover the reading plane without changing its width. Clip covered Graph, history and pointer regions without reflow. Use `ui_motion::revealed_value` for the lower edge. A panel that opens into a known floating placement starts at its calculated height, without a separate lower-edge transition. Animate later height changes while the panel is already visible. Closing and reversal retain continuity until the panel reaches its closed state. Graph expansion retains navigation and collaboration panels. Explicit conversation focus mode can still hide the side panels.

The native composer uses one selected system material. Do not add a second GPUI background fill or a window backdrop below it. Text and controls use the current theme. Measure the native content at the latest parent anchor width before painting so the content, glass contour and input region stay aligned during resize. A full-height navigation preview can cover part of the composer. Mask the covered part of the complete native composite and its pointer region; keep the original material contour, foreground size and draft. Apply that native mask during parent prepaint, before the corresponding frame is painted. Close the temporary preview when the user focuses the composer. Retain the editor and its focus ownership.

When Graph is hidden, an existing conversation with unfinished scoped work can show **Work in progress** above the composer. Use the shared 28-point control height, 14-point Work symbol and intrinsic disclosure motion. Include unfinished tasks and observed native agents from the current conversation's work subtree. Do not insert example work. Limit the expanded list to one quarter of the window height, between two and six control rows, and scroll extra rows. A row opens the existing conversation. Hide the strip in a new conversation or when there is no matching work.

## Reading rhythm

| Relationship | Space |
| --- | --- |
| Separate conversation messages | 20 points. |
| Completed process header and its answer | 6 points. |
| Previous content and working indicator | 20 points. |
| Working indicator and composer | 20 points in total. |
| Working indicator | 24-point status row. |

The composer contributes 8 points to the lower reading gap. Count that contribution when you position a status row. When work ends, release the status row and keep the ordinary reading gap. Folding process details must preserve the final answer and its navigation identity.

## Conversation start

New project Agent and an unstarted personal Assistant use a centered 20-point start heading above the composer when history is confirmed empty. The project Agent shows **What would you like to do?** with its project chooser. The personal Assistant shows **What’s next?** and **A new topic, with the same Assistant.** It has no project chooser. The empty composer keeps its quote placeholder.

The start heading gives way to saved messages, a sending preview, or a retained failed or unconfirmed input. A running response, a question, or a misalignment also keeps the conversation presentation. Missing or unavailable history uses the existing loading or retry presentation. Do not use a passive first-message waiting label for an empty conversation.

## Navigation and attention

Use one compact row rhythm for the left navigation. Pin, Recent, Assistant and Projects have distinct outline symbols. Keep their labels in title case. Do not add total counts.

| Property | Value |
| --- | --- |
| Utility, section, project and conversation row | 28 points. |
| Conversation text | 12.5 points, with a fixed first grapheme. |
| Project title | 12 points, medium weight, normal foreground. |
| Section label | 11 points, medium weight, muted foreground. |
| Relative update time | 10 points, at the far right. |
| Row corner radius | 5 points. |
| Gap between adjacent rows and section headings | 0 points. |
| Divider slot below commands and above Archived | 12 points, with a quiet fading line. |
| Collapsed rail | 40 points, including two 6-point insets. |
| Leading symbol target | 28 points, with a 14-point outline symbol. |
| Hover action target | 24 points; creation and disclosure symbols use a matching 12-point scale. |

The whole section header toggles its content. Sorting remains a secondary hover or keyboard-focus control. Put the creation plus immediately before the disclosure chevron in Assistant, Projects and each project. Both controls retain their hit targets when hidden. An expanded project uses a quiet surface, a short accent mark and an end rule inside its final child row. Its conversation rows have no extra indent or folder symbols.

Keep Search and commands in the same leading slot when the sidebar contracts. It opens the global command window, also available with Command-K. The compact rail retains each section symbol and conversation initial. A pointer dwell of 100 ms temporarily expands the rail as a full-height floating surface, including after an automatic collapse. It temporarily hides a right floating panel without clearing that panel's requested state. Leaving starts a 220 ms close delay; returning cancels that close. Reuse the shared width transition and keep the leading symbols and initials fixed. Opening a section during a hover does not pin the sidebar. The explicit sidebar toggle controls persistent visibility and clears temporary hover state. Relative times and trailing actions do not add width or height to the compact rail.

The sidebar's outer width uses the shared interruptible 200 ms cubic ease-out. Its content retains the complete expanded canvas during opening, closing and a reversal; the outer panel clips that canvas. Keep titles and trailing controls mounted at their final positions until closing reaches the 40-point rail. Do not squeeze the content into intermediate widths or remove labels as soon as the target changes. At the settled compact width, omit project branches and their extra layout space.

Keep the complete sidebar text at its final position and color. `ui_motion::Viewport` measures the actual pane bounds and clips the fixed content at the visible edge, including its shared inner padding. The edge reveals or covers the existing text as a curtain moves. Do not give labels an opacity transition, character counter, normalized reveal fraction or separate clock. Section symbols and conversation initials remain fixed. The Projects directory uses the shared intrinsic disclosure to release its extra space in the compact rail. A reversal changes only the current clipping geometry. Reduced motion selects the target geometry directly.

Use `ui_motion::HoverIntent` for surfaces revealed by pointer intent. The shared tokens set entry dwell to 100 ms and departure grace to 220 ms. Brief crossings cancel entry. Movement inside the same target does not restart its timer. Reentry cancels closing, and an exit from an older target cannot cancel a newer target. Explicit clicks, keyboard actions and dismissal cancel pending pointer work and retain their immediate behavior. The sidebar, Work and Debug islands, and response-statistics popover use this owner. History previews use the shared entry delay rather than a zero-delay tooltip. Ordinary hover ink, contextual row actions and native tooltip lifetime retain their own presentation roles.

A committed pointer intent or explicit surface toggle starts a bounded 400 ms animation window. A visible inactive App can therefore finish the transition without taking foreground focus. This does not change the Full background rendering preference. When that window expires, the ordinary background policy applies again. System reduced motion and VoiceOver still select the target geometry directly. Continuous activity does not renew this window; a new user operation does.

Long titles retain their full text. Hover or keyboard focus starts overflow motion; leaving returns the text smoothly. Each section can enable Always scroll titles. Use shared disclosure, chevron, popover, control-opacity and confirmation-nudge motion. Respect reduced motion and do not animate the complete transcript for a sidebar hover.

Pin, Archive and Delete appear at the fixed far-right edge of a conversation row. The relative time fades while those actions appear. Anchor the owner and time tooltip 8 points outside the row’s trailing edge, aligned to its top; use the shared window-fitting anchor so it cannot cover the row actions. Do not put an opaque backing or gradient seam beneath them. The first Delete click turns the same symbol red and applies the shared nudge. The second click on that symbol confirms deletion. Escape or an outside click cancels. Deletion keeps learned Assistant Memory. Archived rows reuse the native restore and delete paths.

Use restrained warm emphasis for requests that need the user. Derive this state from a pending native request, an unanswered native question, or an explicit user decision. Do not classify attention from a displayed label. Returned agent work and ordinary collaboration remain distinct from requests for a user decision. Window controls use fixed-size icons, with names in tooltips and accessibility labels. Collaboration retains a small pending-count badge when its panel is closed; it adds no visible toolbar label.

## Recovery feedback

Name the cause that the application can establish. A missing selected working folder shows **Working folder is unavailable. Restore it to continue.** Other generic admission failures show **Reconnecting to Codex…** and retain the current technical detail. Account readiness failures keep their specific Accounts guidance. A local folder failure must not imply a network outage.

Keep provider stream retries in native Codex. A retrying stream error does not authorize another user input or a new turn. The isolated native recovery check closes one local SSE response before completion, observes the retry while the original turn stays running, then verifies completion on the same thread and turn. This checks a controlled stream interruption; it does not qualify every physical network failure.

## Theme and controls

On macOS, AppKit owns the window-button and window-sharing geometry. The main window uses a unified compact native toolbar. GPUI follows the measured native control-row center for the leading controls, activity islands and trailing controls. Keep `traffic_light_position` unset. Do not move native window buttons or their parent containers to fit a custom header. AppKit retains blank titlebar dragging.

Control groups use the shared 28-point height, 24-point control targets, 4-point inner spacing and gap, 8-point outer inset and capsule radius, and 5-point small-control radius. Dense navigation retains its 6-point inner inset. Use these role tokens instead of feature-specific values. The native control row sets the top alignment for every titlebar group. Attached material surfaces retain that allocated anchor, including the clipping canvas; the native host must not add a menu edge inset. Independent floating menus use the shared 8-point edge clearance.

Use the current theme tokens for foreground, muted labels, emphasis and status colors. The Ultrafast double bolt uses the theme accent. Its silhouette is shared across the composer; its color is not copied from another application.

Use a quieter color for keys and supporting labels, and the normal foreground for values. Keep settings explanations in complete sentences. Put independent actions in a separate row when their purpose differs: account routing belongs to **New conversations**, while email visibility belongs to the account-list controls.

An unknown model is `—`. An explicit unavailable catalog must not leave a default model name in the input bar. A suggestion is an editable placeholder, wraps within the normal editor and never sends itself. A quote belongs only to a new conversation without history.

## Extend and verify

`AGENTS.md` requires every activity island to use this shared contract. Reuse `Sizing::content`, `arrange`, `Morph`, `MaterialSurface::clip` and the spacing constants. A new variant must update the shared contract and its focused checks. Keep feature-specific content outside the geometry owner. Use the same components in isolated fixtures and the installed application.

The layout tests cover size allocation, fixed canvas bounds, a transition reversal, reduced motion, status spacing and the process-to-answer gap. The native island probe exercises 20 Work and Debug transitions, including four reversals before an opening completes, without a daemon or model request. It checks that the foreground remains at the window origin and that native bounds match the GPUI viewport. The visual-capture binary also accepts `DECODEX_VISUAL_NATIVE_WORKSPACE` with an explicit `DECODEX_FRAME_TRACE` path and isolated `DECODEX_VISUAL_DRAFT_ROOT`. This display-backed workspace probe exercises 12 left, right and combined transitions, including interrupted closing, with the existing Graph fixture. It uses process-local appearance settings. The `DECODEX_VISUAL_NATIVE_COMPOSER` variant resizes a real AppKit composer with the workspace and toggles the right panel. Its trace records the native foreground, viewport and Metal drawable dimensions without a live service. Frame traces count content rebuilds and record CPU draw and submission timing; they do not measure GPU completion or screen scanout. Use the installed app for final mouse, clipping and visual checks.

See [conversation presentation](conversation-presentation.md) and [models and settings](../workflows/models-and-settings.md) for the related product flows.
