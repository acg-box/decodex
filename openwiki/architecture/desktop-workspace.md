---
type: Reference
title: "Desktop workspace and native glass"
description: "Workspace ownership, native menu focus, compositor motion, and consistent status presentation."
tags: [decodex, architecture, desktop, presentation]
sources:
  - id: openwiki-source-75aea95b5b7fd328b3b6a396
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountProfileViews.swift
  - id: openwiki-source-b469e348e65d4cdbb569fb27
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountRows.swift
  - id: openwiki-source-51c6a903a86b67bbf46fe288
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/ResetCardSectionView.swift
  - id: openwiki-source-34d7aa80681e26f05eecbf94
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/StatusPanelController.swift
  - id: openwiki-source-643d559a01e8c78573bfc835
    resource: repo://apps/decodex-gpui/menubar/Tests/DecodexAppTests/StatusPanelLifecycleTests.swift
  - id: openwiki-source-fecafeb22abda7dad3877372
    resource: repo://apps/decodex-gpui/src/account_feedback.rs
  - id: openwiki-source-6512b631b67649d924c16ba3
    resource: repo://apps/decodex-gpui/src/agent_archive.rs
  - id: openwiki-source-3708e6cf9324a8ff1f6c39ae
    resource: repo://apps/decodex-gpui/src/agent_collaboration_tests.rs
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
  - id: openwiki-source-2ee1164d01a09b38a208f1ac
    resource: repo://apps/decodex-gpui/src/agent_markdown.rs
  - id: openwiki-source-ae2ce5cda718ede63f50be9b
    resource: repo://apps/decodex-gpui/src/agent_native_composer.rs
  - id: openwiki-source-1b7747f8ae67c32e5670817d
    resource: repo://apps/decodex-gpui/src/agent_portfolio_tests.rs
  - id: openwiki-source-6a1c55b61be8ac251a493046
    resource: repo://apps/decodex-gpui/src/agent_portfolio.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-15ee466228bdb01892c9678b
    resource: repo://apps/decodex-gpui/src/agent_text_selection.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-fca3688e96d92c09b7ba015f
    resource: repo://apps/decodex-gpui/src/native_material_panel.rs
  - id: openwiki-source-2986b39185cca5c00a29ad1d
    resource: repo://apps/decodex-gpui/src/shell_status.rs
  - id: openwiki-source-6085af9603cb7ac0ff3ef1cf
    resource: repo://crates/decodex-runtime/src/work_portfolio.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
  - id: openwiki-source-c0eb738b42368debe7ef7545
    resource: repo://database/src/works/commands.rs
generated: { by: "codex", at: "2026-10-25T15:11:34.438Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T15:11:34.438Z
---

# Desktop workspace and native glass

## Information hierarchy

The Agent conversation is the primary workspace. The left sidebar selects the overview and projects. The right Collaboration panel keeps recorded exchanges and pending response requests visible across chats. The bottom Dock presents Agent relationships through its graph or list. A historical Program/Factory graph is not the current desktop surface.

Each independent Agent owns its conversation and composer. Messages go to that conversation, including when another Agent appears first in the overview. Assistant and Project Lead responsibilities do not redirect the input. Native approval controls stay in the selected conversation and retain the exact pending request.

Collaboration groups source-backed exchanges by participant pair and expands their recorded text without switching the active chat or draft. A participant control locates that Agent in the graph. Tool completion alone is not delivery, and a context reference is not a message. Missing reply text remains explicit. Pending user requests retain their existing native controls; this panel does not answer them automatically or create a second question store.

Global shortcuts in `shell.rs` are Command-E for the left sidebar, Command-B for the inspector/agent side, and Command-J for the graph. Window controls stay in the global shell. Settings is a separate presentation with bounded scrolling and shared spacing tokens.

## Project Work and Guardian

Work and Guardian are entries in the existing sidebar. Work groups outcomes by Project and can show the same accountable Agent on several rows. Assistant and Project Lead selectors choose existing Agents; they do not create role-specific conversations. The existing conversation, Collaboration panel and Dock remain available.

Expand a Work to read its current requirements, authoritative source, completion criteria, hold reason and resumption condition. Materials open their authorized references. Results show their exact native item or artifact digest. Collaboration Requests show their recipient, issued requirements, scoped Materials, delivery, acceptance, return and assessment. Discuss with owner opens that Agent's conversation. Awaiting Assessment is a derived activity label, not another Work state.

The overview carries headers; selected Work evidence loads separately. Reads are bounded to 100 records per category and disclose additional retained evidence. Large selected records use digest-checked 8 KiB pages with a 32 MiB ceiling. The UI labels cached evidence and offers refresh when the source changed. A missing or stale read does not establish completion.

Pause Work prevents new Decodex contributions and completion. It cannot roll back already-running actions and does not replace native interruption. Work association with a Project does not change the native conversation's working directory or execution permission.

Guardian settings are global: Ask Me or Automatic Review. Full Access remains a separate native permission choice. Desired reviewer settings and actual per-source receipts are visible separately. Review Details is an optional bounded observation: the selected runtime does not provide exact action Review IDs for private reviewer records, so the UI displays Action association unavailable. It does not infer a link to the latest action. Turning details off stops new records; it does not erase retained history.

Only native assistant output and item-free terminal metadata enter retained private records. Raw reasoning frames are excluded. The store retains at most 128 records of 64 KiB each; the snapshot previews the newest 32 and loads the exact selected record on demand. This is not a complete reviewer transcript or proof of a live approval decision.

## Material and focus ownership

`window_material.rs` selects Regular or Clear material. macOS uses native Liquid Glass when supported; the fallback requests GPUI platform blur. Actual transparency depends on platform compositor support. Reduced-transparency preferences and `DECODEX_DISABLE_LIQUID_GLASS` prevent the small native-glass panels.

`native_material_panel.rs` bridges composition only. GPUI still owns UI state, layout and event handlers. The composer is an attached child window created by the main workspace, not another application or service. Settings must not create one. The workspace remains the main window while an attached control can own keyboard focus. See [conversation presentation](conversation-presentation.md#floating-controls-and-motion) for frame ownership and animation policy.

The composer is unavailable for archived or blocked conversations and certain detail/full-screen states. Reopening includes a short layout-settling interval; failures fall back to GPUI presentation. Notification placement and opacity belong to the same native-panel composition boundary.

## Menu focus and account motion

The menu-bar controller opens a borderless nonactivating `NSPanel` and makes that panel key immediately. The first interaction therefore reaches the menu without another focus click. The panel cannot become the main window. Opening it must keep the foreground application unchanged and must not reveal a hidden workspace. Do not replace this boundary with application-wide activation.

`AccountRows` embeds each SwiftUI account row in its own hosting view. AppKit measures the target layout and Core Animation moves and reveals the rendered rows in one transaction. The transparent window canvas grows before expansion and shrinks after collapse. Its top edge stays fixed. The expanded region and the accounts below it move together, without an independent window-height tween or SwiftUI layout on every animation frame.

The row animation requests the current screen's maximum refresh rate. It can retarget from the displayed position; reduced motion applies the destination immediately. This is a scheduling policy, not a guarantee of sustained GPU frame rate. Measure animation cadence and input latency on the target screen under representative load.

See [Accounts and routing](../operations/accounts-and-routing.md) and [Reset Cards](../operations/reset-cards.md) for row actions and independent account details.

## Reading and input

Markdown renders native text, code, lists, and links. Text selection and clipboard operations are read-only. One rendered message shares a selection document across its text blocks. This does not create a selection spanning all messages. Copy controls show short success feedback. Duration and abbreviated token counts remain together at the end of each answer; hovering this row shows detailed usage without changing transcript height.

The history rail follows the reading position. Expanding collaboration exchanges preserves the selected chat and draft. A centered jump-to-latest control represents ongoing work while the user reads older messages. Input controls expose attachments, delivery policy, model/effort, microphone, and live voice without moving execution authority into the UI.

## State and errors

Ordinary operation feedback goes to the notification center. A conversation that cannot send displays its reason in that conversation and hides the composer. Archived history has an explicit Unarchive control. Background archive-read failures preserve the last confirmed state. Explicit failed checks show local feedback.

## Consistent status presentation

Use the same severity color for a status icon and its message. Account login failures use red; recoverable warnings use amber/orange. GPUI informational notices use blue. `PanelPalette` owns the native semantic colors; GPUI uses `ui_theme::ERROR`, `AMBER`, and `BLUE`. Regenerate the corresponding SF Symbol assets with `scripts/macos/generate_workspace_symbols.swift` when those tints change. Status tooltips and native account feedback popovers retain the message color. Ordinary action labels and contrast text inside a colored badge keep their control colors.

The GPUI notification center gives red errors priority over amber warnings and blue information when it selects the bell and badge color. A notice title and its detail use the same notice color. Account warnings use an exclamation icon. Clicking the icon opens the explanation; clicking outside closes it. There is no Dismiss button, and closing the explanation does not clear the warning source. Both native and GPUI account details omit repeated warning text below the activity graph.

## UI wording and punctuation

Use short, direct action or status labels. Do not add a status sentence below an account to repeat its warning icon. Use an ellipsis only for an ongoing operation such as `Loading activity…`. Use complete sentences and normal punctuation for explanations in notifications, popovers and hover text.

Keep each label about one user need. Account warning popovers state what happened and the useful next action. Do not append storage labels to an error or join unrelated messages with a bullet. Describe what happened and the next useful action in plain language. Keep internal error variant names and credential-refresh mechanics out of user-facing account explanations.

## Verification

Use GPUI tests in `shell.rs`, `agent_workspace.rs`, `agent_activity.rs`, `agent_markdown.rs`, `agent_archive.rs` and `settings_surface.rs`. Real macOS acceptance must also check focus, typing, scrolling, panel transitions, and transparency in the signed app. A white or missing automation screenshot alone is not evidence that the user sees a blank window.

The Work portfolio fixtures check Project grouping, scoped evidence, source-bound detail loading, global reviewer controls and explicit limits. The collaboration fixtures check cross-chat visibility, expansion without draft changes, participant location and the distinction between recorded messages, tool completion and context references. These checks do not replace signed-app interaction acceptance.

`StatusPanelLifecycleTests` checks first-open and repeated menu focus, foreground application identity, hidden workspace visibility, and fixed-top resizing. `AccountPanelPresentationTests` checks synchronized row movement, clipping, interruption and reduced motion. The GPUI `account_feedback` test checks first-click opening, outside-click closing, unchanged row height, and retention of the warning source. Its opt-in `DECODEX_MEASURE_ACCOUNT_MOTION=1` presentation sampling separates initial response delay from animation cadence; it does not prove sustained display FPS. `DECODEX_CAPTURE_ACCOUNT_MOTION` selects a screenshot directory for visual review.

See [Conversation presentation and motion](conversation-presentation.md), [Agent coordination](chief-coordination.md) and [Commands and validation](../operations/commands-and-validation.md).

## Retained presentation and drafts

The composer preserves editable drafts and recovered copies. Rich Markdown retains math, Mermaid, weather and source-copy presentation without replacing native execution. The embedded MCP HTML viewer is retired. Ordinary tools and native form prompts remain available. See [conversations](../workflows/conversations-and-recovery.md), [tools](../integrations/tools-plugins-and-apps.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).
