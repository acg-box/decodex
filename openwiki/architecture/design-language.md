---
type: Architecture
title: Interface geometry and motion
description: Reusable layout, activity-island motion, reading spacing and theme rules for the native Decodex desktop.
tags: [desktop, design, motion, layout]
verified:
  - by: openwiki/0.9.0
    at: 2026-11-08T13:22:32.618Z
sources:
  - id: openwiki-source-8037e2358a2c4f9b2c722a11
    resource: repo://AGENTS.md
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
  - id: openwiki-source-48758ba7d154c82885281d2c
    resource: repo://apps/decodex-gpui/src/agent_composer.rs
  - id: openwiki-source-079a15445a0295f222af8069
    resource: repo://apps/decodex-gpui/src/agent_model_settings.rs
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
  - id: openwiki-source-c0e58bc836ac05c982449723
    resource: repo://apps/decodex-gpui/src/agent_workbench.rs
  - id: openwiki-source-e269da8230b6d51390181459
    resource: repo://apps/decodex-gpui/src/agent_workspace_size.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-686aae4ae35c7f99c92e3d9b
    resource: repo://apps/decodex-gpui/src/bin/probes/native_island.rs
  - id: openwiki-source-7b89f5bc6ecd604ca4e97dc2
    resource: repo://apps/decodex-gpui/src/composer_text.rs
  - id: openwiki-source-8baa689fd9c859a6d9a120d5
    resource: repo://apps/decodex-gpui/src/frame_trace.rs
  - id: openwiki-source-fca3688e96d92c09b7ba015f
    resource: repo://apps/decodex-gpui/src/native_material_panel.rs
  - id: openwiki-source-1291f5243fa6c9cb52149bda
    resource: repo://apps/decodex-gpui/src/shell.rs
  - id: openwiki-source-592957b8edd8a689f738c189
    resource: repo://apps/decodex-gpui/src/ui_frame.rs
  - id: openwiki-source-adab7a75320a305721fdd6d2
    resource: repo://apps/decodex-gpui/src/ui_hover_intent.rs
  - id: openwiki-source-ccd97a4d4ef8dfe9b724e7ce
    resource: repo://apps/decodex-gpui/src/ui_live_activity.rs
  - id: openwiki-source-afbf2d30c0979a844373d8e5
    resource: repo://apps/decodex-gpui/src/ui_motion.rs
  - id: openwiki-source-9e7fc8b7290695ff7afa8373
    resource: repo://apps/decodex-gpui/src/ui_overflow_label.rs
  - id: openwiki-source-8f7f43de2b98b69bb43506d7
    resource: repo://apps/decodex-gpui/src/ui_performance_readout.rs
  - id: openwiki-source-6d7ff71d731da7268890d765
    resource: repo://apps/decodex-gpui/src/ui_performance.rs
  - id: openwiki-source-570564020f98df7a4d6d0b61
    resource: repo://apps/decodex-gpui/src/ui_theme.rs
  - id: openwiki-source-343d35c489d39a7a672412a2
    resource: repo://apps/decodex-gpui/src/ui_working.rs
  - id: openwiki-source-f02ae41e2bd15dda85dc86b9
    resource: repo://codex-runtime.lock.json
  - id: openwiki-source-a490ea3fbcddb9c007caf196
    resource: repo://crates/decodex-runtime/src/account_launch/agent_process_native_context_tests.rs
generated: { by: "codex", at: "2026-11-08T13:22:32.618Z" }
---

# Interface geometry and motion

Use shared geometry and presentation components when you add a desktop surface. `ui_theme` owns spacing, type and color tokens. `ui_live_activity` owns activity-island size tiers and motion. A feature owns its data and actions. It does not need another animation system.

## Activity islands

Work and Debug use the same geometry contract.

| Property | Rule |
| --- | --- |
| Collapsed height | 28 points, from the shared control-group height. |
| Minimum width | At least 72 points; round measured content up to an 8-point grid. |
| Compact width | At least 208 points; round wider content up to a 16-point grid. |
| Expanded width | At least 288 points; round wider content up to a 32-point grid. |
| Space between islands | 6 points. |
| Corner radius | 14 points when collapsed, increasing to 20 points when expanded. |
| Transition | 240 ms, with a cubic ease-out and no overshoot. |
| Detail appearance | Fade details during the final 45 percent of the height transition. |

Only one activity expands at a time. Two activities retain their resting slots and inner edges; they expand outward. A single activity remains centered. If space cannot fit the expanded tier, use the compact or minimum tier. Do not squeeze the expanded detail grid into a narrow header.

`Morph` uses one clock for width and height. A reversal starts at the current visible size. Reduced motion selects the target size directly. Round visible dimensions to physical pixels using the current display scale.

Keep the native drawable at the complete content size and at the window origin during a transition. `MaterialSurface::clip` puts the foreground beside the material and applies a rounded foreground mask. The material contour and mask change together. The mask updates disable implicit Core Animation actions; `Morph` owns the transition. Do not resize the Metal drawable on each animation frame. The input region follows the visible contour, so the clear area outside an island does not block controls below it.

Lay out detail text and metric columns at the complete content width. Clip that layout during opening and closing. Values retain one view identity and move along a defined path. Labels follow their corresponding values and fade in only when the complete field fits with its required separation. A narrow intermediate header must not paint secondary text over its main value. Do not reveal a label by repeatedly changing its text width or paint two copies of the same value at different positions.

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

Keep Search and commands in the same leading slot when the sidebar contracts. It opens the global command window, also available with Command-K. The compact rail retains each section symbol and conversation initial. A pointer dwell of 180 ms temporarily expands the rail at workspace widths above 1,000 points. Leaving starts a 220 ms close delay; returning cancels that close. Reuse the shared width transition and keep the leading symbols and initials fixed. Opening a section during a hover does not pin the sidebar. The explicit sidebar toggle controls persistent visibility and clears temporary hover state. Relative times and trailing actions do not add width or height to the compact rail.

The sidebar's outer width uses the shared interruptible 200 ms cubic ease-out. Its content retains the complete expanded canvas during opening, closing and a reversal; the outer panel clips that canvas. Keep titles and trailing controls mounted at their final positions until closing reaches the 40-point rail. Do not squeeze the content into intermediate widths or remove labels as soon as the target changes. At the settled compact width, omit project branches and their extra layout space.

Use `ui_motion::PanelReveal` to derive detail opacity from the visible width. Labels, title suffixes, times and trailing controls fade with a smooth curve. Section symbols and conversation initials keep their position and full opacity. Fade the complete project tree because project names have no compact initial. Detail opacity is zero when the expanded canvas enters or leaves the rail, and one at the full width. A reversal uses the current width and opacity; it does not restart a separate fade. Reduced motion uses the target width and opacity directly.

Use `ui_motion::HoverIntent` for surfaces revealed by pointer intent. The shared tokens set entry dwell to 180 ms and departure grace to 220 ms. Brief crossings cancel entry. Movement inside the same target does not restart its timer. Reentry cancels closing, and an exit from an older target cannot cancel a newer target. Explicit clicks, keyboard actions and dismissal cancel pending pointer work and retain their immediate behavior. The sidebar, Work and Debug islands, and response-statistics popover use this owner. History previews use the shared entry delay rather than a zero-delay tooltip. Ordinary hover ink, contextual row actions and native tooltip lifetime retain their own presentation roles.

A committed pointer intent or explicit surface toggle starts a bounded 400 ms animation window. A visible inactive App can therefore finish the transition without taking foreground focus. This does not change the Full background rendering preference. When that window expires, the ordinary background policy applies again. System reduced motion and VoiceOver still select the target geometry directly. Continuous activity does not renew this window; a new user operation does.

Long titles retain their full text. Hover or keyboard focus starts overflow motion; leaving returns the text smoothly. Each section can enable Always scroll titles. Use shared disclosure, chevron, popover, control-opacity and confirmation-nudge motion. Respect reduced motion and do not animate the complete transcript for a sidebar hover.

Pin, Archive and Delete appear at the fixed far-right edge of a conversation row. The relative time fades while those actions appear. Anchor the owner and time tooltip 8 points outside the row’s trailing edge, aligned to its top; use the shared window-fitting anchor so it cannot cover the row actions. Do not put an opaque backing or gradient seam beneath them. The first Delete click turns the same symbol red and applies the shared nudge. The second click on that symbol confirms deletion. Escape or an outside click cancels. Deletion keeps learned Assistant Memory. Archived rows reuse the native restore and delete paths.

Use restrained warm emphasis for requests that need the user. Derive this state from a pending native request, an unanswered native question, or an explicit user decision. Do not classify attention from a displayed label. Returned agent work and ordinary collaboration remain distinct from requests for a user decision. Window controls use fixed-size icons, with names in tooltips and accessibility labels. Collaboration retains a small pending-count badge when its panel is closed; it adds no visible toolbar label.

## Recovery feedback

Name the cause that the application can establish. A missing selected working folder shows **Working folder is unavailable. Restore it to continue.** Other generic admission failures show **Reconnecting to Codex…** and retain the current technical detail. Account readiness failures keep their specific Accounts guidance. A local folder failure must not imply a network outage.

Keep provider stream retries in native Codex. A retrying stream error does not authorize another user input or a new turn. The isolated native recovery check closes one local SSE response before completion, observes the retry while the original turn stays running, then verifies completion on the same thread and turn. This checks a controlled stream interruption; it does not qualify every physical network failure.

## Theme and controls

Use the current theme tokens for foreground, muted labels, emphasis and status colors. The Ultrafast double bolt uses the theme accent. Its silhouette is shared across the composer; its color is not copied from another application.

Use a quieter color for keys and supporting labels, and the normal foreground for values. Keep settings explanations in complete sentences. Put independent actions in a separate row when their purpose differs: account routing belongs to **New conversations**, while email visibility belongs to the account-list controls.

An unknown model is `—`. An explicit unavailable catalog must not leave a default model name in the input bar. A suggestion is an editable placeholder, wraps within the normal editor and never sends itself. A quote belongs only to a new conversation without history.

## Extend and verify

`AGENTS.md` requires every activity island to use this shared contract. Reuse `Sizing::content`, `arrange`, `Morph`, `MaterialSurface::clip` and the spacing constants. A new variant must update the shared contract and its focused checks. Keep feature-specific content outside the geometry owner. Use the same components in isolated fixtures and the installed application.

The layout tests cover size allocation, fixed canvas bounds, a transition reversal, reduced motion, status spacing and the process-to-answer gap. The native island probe exercises 20 Work and Debug transitions, including four reversals before an opening completes, without a daemon or model request. It checks that the foreground remains at the window origin and that native bounds match the GPUI viewport. Its frame trace records CPU draw and submission timing; it does not measure GPU completion or screen scanout. Use the installed app for final mouse, clipping and visual checks.

See [conversation presentation](conversation-presentation.md) and [models and settings](../workflows/models-and-settings.md) for the related product flows.
