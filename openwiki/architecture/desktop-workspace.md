---
type: Reference
title: "Desktop workspace and native glass"
description: "Workspace ownership, native menu focus, compositor motion, and consistent status presentation."
tags: [decodex, architecture, desktop, presentation]
verified:
  - by: openwiki/0.6.1
    at: 2026-09-30T17:38:04.493Z
sources:
  - id: openwiki-source-b469e348e65d4cdbb569fb27
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountRows.swift
  - id: openwiki-source-51c6a903a86b67bbf46fe288
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardSectionView.swift
  - id: openwiki-source-34d7aa80681e26f05eecbf94
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/StatusPanelController.swift
  - id: openwiki-source-643d559a01e8c78573bfc835
    resource: repo://apps/decodex-gpui/menubar/Tests/DecodexAppTests/StatusPanelLifecycleTests.swift
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
  - id: openwiki-source-2986b39185cca5c00a29ad1d
    resource: repo://apps/decodex-gpui/src/shell_status.rs
generated: { by: "codex", at: "2026-09-30T17:38:04.493Z" }
---

# Desktop workspace and native glass

## Information hierarchy

The Agent conversation is the primary workspace. The left sidebar selects the overview and projects; the right agent tree describes ownership; the bottom graph describes work dependencies and reports. A historical Program/Factory graph is not the current desktop surface.

Global shortcuts in `shell.rs` are Command-E for the left sidebar, Command-B for the inspector/agent side, and Command-J for the graph. Window controls stay in the global shell. Settings is a separate presentation with bounded scrolling and shared spacing tokens.

## Material and focus ownership

`window_material.rs` selects Regular or Clear material. macOS uses native Liquid Glass when supported; the fallback requests GPUI platform blur. Actual transparency depends on platform compositor support. Reduced-transparency preferences and `DECODEX_DISABLE_LIQUID_GLASS` prevent the small native-glass panels.

`native_glass_panel.rs` bridges composition only. GPUI still owns UI state, layout and event handlers. The composer is an attached child window created by the main workspace, not another application or service. Settings must not create one. The workspace remains the main window while an attached control can own keyboard focus. Animation scheduling uses the key child's display cadence to avoid throttling a focused composer's parent.

The composer is unavailable for archived or blocked conversations and certain detail/full-screen states. Reopening includes a short layout-settling interval; failures fall back to GPUI presentation. Notification placement and opacity belong to the same native-panel composition boundary.

## Menu focus and account motion

The menu-bar controller opens a borderless nonactivating `NSPanel` and makes that panel key immediately. The first interaction therefore reaches the menu without another focus click. The panel cannot become the main window. Opening it must keep the foreground application unchanged and must not reveal a hidden workspace. Do not replace this boundary with application-wide activation.

`AccountRows` embeds each SwiftUI account row in its own hosting view. AppKit measures the target layout and Core Animation moves and reveals the rendered rows in one transaction. The transparent window canvas grows before expansion and shrinks after collapse. Its top edge stays fixed. The expanded region and the accounts below it move together, without an independent window-height tween or SwiftUI layout on every animation frame.

The row animation requests the current screen's maximum refresh rate. It can retarget from the displayed position; reduced motion applies the destination immediately. This is a scheduling policy, not a guarantee of sustained GPU frame rate. Measure animation cadence and input latency on the target screen under representative load.

See [Accounts and routing](../operations/accounts-and-routing.md) and [Reset Cards](../operations/reset-cards.md) for row actions and independent account details.

## Reading and input

Markdown renders native text, code, lists, and links. Text selection and clipboard operations are read-only. Selection currently belongs to each rendered text block; this is not one continuous selection across all messages. Copy controls show short success feedback. Duration and abbreviated token counts remain together at the end of each answer; hovering this row shows detailed usage without changing transcript height.

The history rail follows the reading position. Expanding connection details must preserve the anchor. A centered jump-to-latest control represents ongoing work while the user reads older messages. Input controls expose attachments, delivery policy, model/effort, microphone, and live voice without moving execution authority into the UI.

## State and errors

Ordinary operation feedback goes to the notification center. A conversation that cannot send displays its reason in that conversation and hides the composer. Archived history has an explicit Unarchive control. Background archive-read failures preserve the last confirmed state. Explicit failed checks show local feedback.

## Consistent status presentation

Use the same severity color for a status icon and its message. Account login failures use red; recoverable warnings use amber/orange. GPUI informational notices use blue. `PanelPalette` owns the native semantic colors; GPUI uses `ui_theme::ERROR`, `AMBER`, and `BLUE`. Regenerate the corresponding SF Symbol assets with `scripts/macos/generate_workspace_symbols.swift` when those tints change. Status tooltips and native account feedback popovers retain the message color. Ordinary action labels and contrast text inside a colored badge keep their control colors.

The GPUI notification center gives red errors priority over amber warnings and blue information when it selects the bell and badge color. A notice title and its detail use the same notice color. Keep full diagnostics in their contextual detail surface; the inline cached-activity status occupies at most one line.

## Verification

Use GPUI tests in `shell.rs`, `agent_workspace.rs`, `agent_activity.rs`, `agent_markdown.rs`, `agent_archive.rs` and `settings_surface.rs`. Real macOS acceptance must also check focus, typing, scrolling, panel transitions, and transparency in the signed app. A white or missing automation screenshot alone is not evidence that the user sees a blank window.

`StatusPanelLifecycleTests` checks first-open and repeated menu focus, foreground application identity, hidden workspace visibility, and fixed-top resizing. `AccountPanelPresentationTests` checks synchronized row movement, clipping, interruption and reduced motion. Its opt-in `DECODEX_MEASURE_ACCOUNT_MOTION=1` presentation sampling separates initial response delay from animation cadence; it does not prove sustained display FPS. `DECODEX_CAPTURE_ACCOUNT_MOTION` selects a screenshot directory for visual review.

See [Conversation presentation and motion](conversation-presentation.md), [Agent coordination](chief-coordination.md) and [Commands and validation](../operations/commands-and-validation.md).

## Retained presentation and drafts

The composer preserves editable drafts and recovered copies. Rich Markdown retains math, Mermaid, weather and source-copy presentation without replacing native execution. The embedded MCP HTML viewer is retired. Ordinary tools and native form prompts remain available. See [conversations](../workflows/conversations-and-recovery.md), [tools](../integrations/tools-plugins-and-apps.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).
