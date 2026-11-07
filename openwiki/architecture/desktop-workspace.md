---
type: Reference
title: "Desktop workspace and native glass"
description: "Workspace ownership, native menu focus, compositor motion, and consistent status presentation."
tags: [decodex, architecture, desktop, presentation]
verified:
  - by: openwiki/0.9.0
    at: 2026-11-07T07:31:20.360Z
sources:
  - id: openwiki-source-98e7b23c4cc276d20fcb4649
    resource: repo://apps/decodex-gpui/menubar/Sources/DecodexApp/AccountControlViews.swift
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
  - id: openwiki-source-bf03f317e64cfefd11f6ea83
    resource: repo://apps/decodex-gpui/src/agent_attached_branches.rs
  - id: openwiki-source-b243cfb8ae3053f8047fdab0
    resource: repo://apps/decodex-gpui/src/agent_capabilities.rs
  - id: openwiki-source-d9c9b48c73ee4a2adc90f01c
    resource: repo://apps/decodex-gpui/src/agent_collaboration_cases.rs
  - id: openwiki-source-3708e6cf9324a8ff1f6c39ae
    resource: repo://apps/decodex-gpui/src/agent_collaboration_tests.rs
  - id: openwiki-source-2a6989c7fa25b9681df71f95
    resource: repo://apps/decodex-gpui/src/agent_collaboration.rs
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
  - id: openwiki-source-02f8a02cdfa0616877cdad62
    resource: repo://apps/decodex-gpui/src/agent_list_status.rs
  - id: openwiki-source-2ee1164d01a09b38a208f1ac
    resource: repo://apps/decodex-gpui/src/agent_markdown.rs
  - id: openwiki-source-9c68c30b2ff24ddf9a764d74
    resource: repo://apps/decodex-gpui/src/agent_memory.rs
  - id: openwiki-source-1e0b3343cd024646618a1f95
    resource: repo://apps/decodex-gpui/src/agent_native_agents.rs
  - id: openwiki-source-ae2ce5cda718ede63f50be9b
    resource: repo://apps/decodex-gpui/src/agent_native_composer.rs
  - id: openwiki-source-e59b6fade5576eca54cd30f7
    resource: repo://apps/decodex-gpui/src/agent_native_menu.rs
  - id: openwiki-source-541c96314da57ad5afa1835e
    resource: repo://apps/decodex-gpui/src/agent_navigation_sidebar.rs
  - id: openwiki-source-c6ac7d6b2fdeb458faab2c3a
    resource: repo://apps/decodex-gpui/src/agent_permissions.rs
  - id: openwiki-source-1b7747f8ae67c32e5670817d
    resource: repo://apps/decodex-gpui/src/agent_portfolio_tests.rs
  - id: openwiki-source-6a1c55b61be8ac251a493046
    resource: repo://apps/decodex-gpui/src/agent_portfolio.rs
  - id: openwiki-source-0eea3c2bab20e4a9d8e3d80a
    resource: repo://apps/decodex-gpui/src/agent_prompt_confirm_wire_tests.rs
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-eaafddc6420cb2e60f559a55
    resource: repo://apps/decodex-gpui/src/agent_relation_layout.rs
  - id: openwiki-source-a94720ef223006f5ecee5f55
    resource: repo://apps/decodex-gpui/src/agent_relation_scopes.rs
  - id: openwiki-source-5da2f5dd568514f1f464d177
    resource: repo://apps/decodex-gpui/src/agent_surface.rs
  - id: openwiki-source-15ee466228bdb01892c9678b
    resource: repo://apps/decodex-gpui/src/agent_text_selection.rs
  - id: openwiki-source-d57edbe5de23ebee985fc88b
    resource: repo://apps/decodex-gpui/src/agent_timeline_inputs.rs
  - id: openwiki-source-691432bb61082c358b6f9c24
    resource: repo://apps/decodex-gpui/src/agent_timeline.rs
  - id: openwiki-source-51a4755f3c4ddd78511e5c8e
    resource: repo://apps/decodex-gpui/src/agent_tree.rs
  - id: openwiki-source-0d053d4de2f668168f982629
    resource: repo://apps/decodex-gpui/src/agent_usage_evidence.rs
  - id: openwiki-source-32bb36aae256aa57e6e82bd0
    resource: repo://apps/decodex-gpui/src/agent_voice.rs
  - id: openwiki-source-3fa6d6c49e3542409542045b
    resource: repo://apps/decodex-gpui/src/agent_work_browser.rs
  - id: openwiki-source-e269da8230b6d51390181459
    resource: repo://apps/decodex-gpui/src/agent_workspace_size.rs
  - id: openwiki-source-5d032c6be684964aa2c401fa
    resource: repo://apps/decodex-gpui/src/agent_workspace.rs
  - id: openwiki-source-f339a4d1910db63ca305c2df
    resource: repo://apps/decodex-gpui/src/assistant_settings.rs
  - id: openwiki-source-ae991159453be2ee0c611ac1
    resource: repo://apps/decodex-gpui/src/bundled_daemon.rs
  - id: openwiki-source-e8596cc5aac62aa16bde932b
    resource: repo://apps/decodex-gpui/src/component_runtime.rs
  - id: openwiki-source-fca3688e96d92c09b7ba015f
    resource: repo://apps/decodex-gpui/src/native_material_panel.rs
  - id: openwiki-source-a2f254020dd8eaabd6366c91
    resource: repo://apps/decodex-gpui/src/session_settings.rs
  - id: openwiki-source-0813b2e35e2133631c0aa4db
    resource: repo://apps/decodex-gpui/src/settings_components.rs
  - id: openwiki-source-de666753614ba03227181179
    resource: repo://apps/decodex-gpui/src/settings_power.rs
  - id: openwiki-source-a16804a567889414868e5d67
    resource: repo://apps/decodex-gpui/src/settings_surface.rs
  - id: openwiki-source-175eb5a6c5aac2391d3a3716
    resource: repo://apps/decodex-gpui/src/settings_themes.rs
  - id: openwiki-source-cfb92e5db8b3a39d51f6a057
    resource: repo://apps/decodex-gpui/src/settings_updates.rs
  - id: openwiki-source-2dfc8cb7dbdb75c07469c4c3
    resource: repo://apps/decodex-gpui/src/shell_account_activity.rs
  - id: openwiki-source-5afd606185b0f01d8fc4f429
    resource: repo://apps/decodex-gpui/src/shell_diagnostics.rs
  - id: openwiki-source-4b6e253ef76717138b4dd66e
    resource: repo://apps/decodex-gpui/src/shell_reset_cards.rs
  - id: openwiki-source-2986b39185cca5c00a29ad1d
    resource: repo://apps/decodex-gpui/src/shell_status.rs
  - id: openwiki-source-1291f5243fa6c9cb52149bda
    resource: repo://apps/decodex-gpui/src/shell.rs
  - id: openwiki-source-fe671b38d0336d4630edcd46
    resource: repo://apps/decodex-gpui/src/ui_disclosure.rs
  - id: openwiki-source-2f256c95c226c1326d164ad3
    resource: repo://apps/decodex-gpui/src/ui_feedback.rs
  - id: openwiki-source-afbf2d30c0979a844373d8e5
    resource: repo://apps/decodex-gpui/src/ui_motion.rs
  - id: openwiki-source-546812ff3637690f4a184b89
    resource: repo://apps/decodex-gpui/src/ui_preferences.rs
  - id: openwiki-source-570564020f98df7a4d6d0b61
    resource: repo://apps/decodex-gpui/src/ui_theme.rs
  - id: openwiki-source-69e9fd7efd8c3cd616184e27
    resource: repo://crates/decodex-components/src/ragflow_service.rs
  - id: openwiki-source-c29d70b5569f29b7122f2f6b
    resource: repo://crates/decodex-runtime/src/native_agents.rs
  - id: openwiki-source-6085af9603cb7ac0ff3ef1cf
    resource: repo://crates/decodex-runtime/src/work_portfolio.rs
  - id: openwiki-source-58c0167d31c3e40dc89591ab
    resource: repo://database/src/agent_fork.rs
  - id: openwiki-source-3f60f3a3aae45b3e3b355d3e
    resource: repo://database/src/assistant.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
  - id: openwiki-source-c0eb738b42368debe7ef7545
    resource: repo://database/src/works/commands.rs
  - id: openwiki-source-8fe6f1557e5d45d88b34bd0a
    resource: repo://scripts/macos/generate_workspace_symbols.swift
generated: { by: "codex", at: "2026-11-07T07:31:20.360Z" }
---

# Desktop workspace and native glass

## Information hierarchy

The workspace has three responsibilities. The left sidebar opens personal Assistant topics, persistent Work and secondary project chats. The center retains the selected conversation and its composer. The right Collaboration panel shows source-backed requests and exchanges across chats. A selected Work does not limit global pending requests to that Work.

**My Assistant** opens the canonical personal conversation. **New conversation** creates another personal topic with a fresh context and the same Assistant Memory and configuration. It does not create a project Agent. Each topic keeps its own draft, and the conversation header shows its current topic name. **Discuss new work** starts another personal topic. The Work section shows up to eight active outcome headers and provides **View all work** and **Find project chats**. The footer provides **Archived conversations**, **New project Agent** and **Settings**. The archive label describes its actual scope; it is not a general history browser.

Collaboration shows **Needs you** before **Between agents**. Pending native requests, unanswered questions and explicit user decisions need a response; a returned result still needs its original assessment and is not automatically accepted. Opening an exchange's details keeps the active conversation and draft. Explicit **Open request**, **Open question** and **Open review** actions lead to the owning conversation, where its native response controls retain authority. The toolbar keeps the pending count when the right panel is closed. The Graph remains available through its existing control and shortcut, but starts closed.

The six Settings sections are General, Agents, Appearance, Components, Accounts and Diagnostics. They use the same 620-point content column and shared rows. General contains the app version and native Check for updates action; Updates has no separate navigation entry. New conversations uses two setting rows: Permissions shows the selected mode's meaning and opens the shared permission menu, and Speed reuses the existing segmented control with usage for the selected speed. General groups automatic recaps and sleep prevention under Background.

Settings reuse the App's normal body and control typography. Section titles, setting names, values and explanations retain their existing sizes. Ordinary setting names, component names and field labels use normal weight; section titles use medium weight and muted text. Related controls share one quiet surface; rows do not each acquire a separate card. Necessary descriptions remain visible. The permission menu shows all four explanations and the native inheritance rule directly. Native options use one flat list under Context defaults, with one shared-scope description. Assistance contains suggestion and Guardian controls. Model fields place each label above its input. Model verification and Add account actions use the shared filled button. Help text explains a choice, scope, cost, data effect, compatibility limit or recovery action. Self-explanatory controls do not repeat their labels in captions.

The permission picker keeps its open state and focus handles in the window. Its visible trailing edge aligns with the trigger, excluding the outer shadow gutter. Arrow keys cycle within its options. Escape closes the menu and restores focus to its trigger. Tab and Shift-Tab dismiss the menu and continue keyboard navigation. Settings remains open after repeated Escape. Command-W or the window close button closes Settings, including while the menu is open. The picker reuses the existing permission rows; it does not own conversation permissions.

Settings navigation uses its final layout in the first frame. User-triggered details in Accounts, Components, Diagnostics and Tool catalog cache use the shared 200 ms disclosure animation. The catalog panel uses the same presentation in Agent preferences. Settings destinations bypass the page-arrival fade, and the standalone Settings window configures its material before activation. Button, switch and chevron feedback remain independent of page geometry. Regression tests check stable first entry, intermediate expansion and collapse frames, interruption, clipping, live content changes and the absence of idle frame requests.

Settings and conversation disclosures animate their open proportion. The first opening frame resolves the available width at zero height. During motion, the component measures current content at that width before laying out siblings and clips both prepaint and paint to the revealed bounds. The same flow box includes child margins during measurement and settled layout. It does not reuse a previous frame's content height. Already open details mount at full height. Settled content uses normal layout, so nested size changes do not replay expansion. Completed collapse removes the element from layout, including parent gaps. Measurements do not notify the model. Motion uses the GPUI clock, can reverse from the displayed position and respects reduced motion. Arrival on other workspace destinations changes opacity without moving content or hit targets.

Ordinary preferences use shared group surfaces and one divided-row helper. Accounts uses one surface for its compact list. Each account summary places its identity, a fixed-width quota column and management actions on one compact row. One or two quota windows use the same summary height. Only the summary has the disclosure click target and hand cursor; expanded reading areas keep the ordinary cursor. Not-applicable quota windows are omitted; unknown usage retains its row. Expanded activity keeps two columns of labels and values, with current and best streak together, followed by the daily chart and a separate Reset cards area. Metric labels and values share the quota text size and muted color; values use tabular figures without extra size or weight. The daily chart uses a subdued accent. Component names, versions and lifecycle actions share one row in a divided list. Version rows retain the complete reported value, including its original prefix, punctuation, build metadata and commit suffix. Receipts and the UI retain the server value directly. Expanded component, MCP and Diagnostics content uses an inset detail surface with distinct headings, values and guidance. Feedback stays with the operation that owns it.

Diagnostics first shows one status line in normal Settings text and three group summaries. An unchecked Codex status does not add a second caption that repeats the visible Run checks action. Necessary connection and recovery information stays in the same text flow or the specific issue notice. Technical details retains every individual result and cached-report warning. A uniform group shows its state once beside the heading; mixed groups keep per-item states. Missing and unchecked results remain explicit. Current summaries do not use stale reports. Native process values use aligned columns with explicit missing-value labels.

Appearance lays out all theme previews in the page's one scroll area. Theme names can wrap, and the gallery does not capture a separate inner scroll. Color scheme help appears when System is selected.

Visual references: [Zed Settings design](https://zed.dev/blog/settings-ui), [Raycast Settings](https://manual.raycast.com/settings) and [Linear preferences](https://linear.app/docs/account-preferences).

Codex prepares automatically in the background at local App startup. Components shows the resulting status without a permanent Verify button or a generic Retry action. An interrupted first download returns to Install. Reinstall is available for an installed private runtime and uses the existing preparation owner. Complete operation errors remain in the expanded details. The App does not repair an external container runtime.

The Agent conversation is the primary workspace. The left sidebar selects the overview and projects. The right Collaboration panel keeps recorded exchanges and pending response requests visible across chats. The bottom Dock presents Agent relationships through its graph or list.

Each independent Agent owns its conversation and composer. Messages go to that conversation, including when another Agent appears first in the overview. Assistant and Project Lead responsibilities do not redirect the input. Native approval controls stay in the selected conversation and retain the exact pending request.

Collaboration groups source-backed exchanges by participant pair and expands their recorded text without switching the active chat or draft. A participant control locates that Agent in the graph. Tool completion alone is not delivery, and a context reference is not a message. Missing reply text remains explicit. Pending user requests retain their existing native controls; this panel does not answer them automatically or create a second question store.

Global shortcuts in `shell.rs` are Command-E for the left sidebar, Command-B for the inspector/agent side, and Command-J for the graph. Window controls stay in the global shell. Settings is a separate presentation with bounded scrolling and shared spacing tokens.

The Graph groups Agents by recorded Project membership. A quiet bottom rail and a stable caption identify that scope. Selecting a Project or one of its Agents highlights the rail and adds upper-corner marks. Tree, Radial and Grouped keep clearance between Projects. Point overview omits small captions. These marks do not move an Agent to another Project or change ownership.

Settings > Agents supplies permission and speed defaults for new independent conversations. The input-bar shield selects the current Agent's native permission mode; speed stays inside the model menu. Existing conversations retain their own settings, and native Subagents follow native inheritance rather than a Decodex broadcast.

Shift-Esc toggles Graph or conversation fullscreen according to the active pane. Pane capture tracks clicks across cached Graph layers, and the native composer forwards actions to the persistent shell. Graph fullscreen hides both sidebars and restores their saved visibility and width preferences on exit.

The conversation header groups each source with its open Side chats. Use **+** to create and open a native attached branch from the selected conversation after source preparation. Creation needs no second confirmation and sends no message. Switching tabs preserves each conversation's draft and reading position; closing a tab retains the native branch. See [Side chat tabs](conversation-presentation.md#side-chat-tabs).

## Project Work and review details

Work groups outcomes by Project and can show the same accountable Agent on several rows. Assistant and Project Lead selectors choose existing Agents; they do not create role-specific conversations. Selected Work evidence stays with the existing Work owner. The conversation, global Collaboration panel and Graph remain available. Guardian observation settings and per-conversation evidence retain their existing owners; Guardian is not a permanent section in the new sidebar.

Expand a Work to read its current requirements, authoritative source, completion criteria, hold reason and resumption condition. Materials open their authorized references. Results show their exact native item or artifact digest. Collaboration Requests show their recipient, issued requirements, scoped Materials, delivery, acceptance, return and assessment. Discuss with owner opens that Agent's conversation. Awaiting Assessment is a derived activity label, not another Work state.

The overview carries headers; selected Work evidence loads separately. Reads are bounded to 100 records per category and disclose additional retained evidence. Large selected records use digest-checked 8 KiB pages with a 32 MiB ceiling. The UI labels cached evidence and offers refresh when the source changed. A missing or stale read does not establish completion.

Pause Work prevents new Decodex contributions and completion. It cannot roll back already-running actions and does not replace native interruption. Work association with a Project does not change the native conversation's working directory or execution permission.

Guardian retains review records for Agents and Projects. Its global **Save review details** switch controls optional bounded observation, not conversation permissions. The selected runtime does not provide exact action Review IDs for private reviewer records, so the UI displays Action association unavailable. It does not infer a link to the latest action. Turning details off stops new records; it does not approve requests or erase retained history.

Only native assistant output and item-free terminal metadata enter retained private records. Raw reasoning frames are excluded. The store retains at most 128 records of 64 KiB each; the snapshot previews the newest 32 and loads the exact selected record on demand. This is not a complete reviewer transcript or proof of a live approval decision.

## List status filters

The existing conversation browser and Work list each offer status filters and **All statuses**. Each view keeps its own selection through the host-local presentation preference owner. Status filtering combines with the existing Project and text search filters. A status that no longer has a matching row remains selected and visible until the user clears it.

Managed conversation labels come from the existing status projection, native child rows keep their reported labels, and Work uses its current business state. Filtering does not change those facts, send a command, move a conversation or clear a draft. Work remains grouped by Project, and exact row opening and selected evidence keep their existing owners. See [the list workflow](../workflows/conversations-and-recovery.md#filter-existing-lists-by-status) and [qualification limits](../testing/upstream-acceptance-boundaries.md#list-status-filter-qualification).

## Read-only evidence views

Agent preferences includes **Memory summary (read-only)** for the current native root conversation. It reads the selected native summary without starting or resuming a conversation. **Refresh** and **Copy** are explicit actions. Empty native memory can mean disabled, missing, unreadable or blank memory. A source or binding change removes the old view. The desktop does not edit, regenerate or interpret the summary as instructions.

**Actual usage** is available in Agent preferences and expanded Work details. The panel shows known whole-turn tokens, recorded duration, coverage counts and bounded exact source examples. Unknown counters stay unknown. Work uses saved turn references across retained requirements; linked turns can contain other activity, and known shared turns are excluded from linked-turn totals. Recorded durations can overlap. Copy usage evidence exports this observed view after an explicit action.

The existing Collaboration panel includes retained question answers and independent contribution cycles. Exact reply delivery and receiving-turn outcome remain distinct from question resolution or Work assessment. Repair cycles preserve their own sources. See [Agent coordination](chief-coordination.md#read-only-collaboration-evidence-and-usage) for the joins and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md#selected-read-only-views) for verification limits.

Agent preferences also includes **Side chats**. It lists the newest 100 saved Decodex branch receipts for the exact source conversation. **Open** uses the existing conversation navigation and preserves the source draft. An uncertain creation stays unconfirmed; the list does not repeat it. An archived branch must be restored in Codex before refresh can open it. See [conversation recovery](../workflows/conversations-and-recovery.md) for creation and recovery.

## Material and focus ownership

`window_material.rs` selects Regular or Clear material. macOS uses native Liquid Glass when supported; the fallback requests GPUI platform blur. Actual transparency depends on platform compositor support. Reduced-transparency preferences and `DECODEX_DISABLE_LIQUID_GLASS` prevent the small native-glass panels.

`native_material_panel.rs` bridges composition only. GPUI still owns UI state, layout and event handlers. The composer is an attached child window created by the main workspace, not another application or service. Settings must not create one. The workspace remains the main window while an attached control can own keyboard focus. See [conversation presentation](conversation-presentation.md#floating-controls-and-motion) for frame ownership and animation policy.

Archived and blocked conversations keep the normal composer geometry with sending disabled and the draft retained. A locked conversation with a readable, currently bound native prompt offers **Fork**. This creates and opens a native branch directly without taking over the original writer. Certain detail/full-screen states omit the composer. Reopening includes a short layout-settling interval; failures fall back to GPUI presentation. Notification placement and opacity belong to the same native-panel composition boundary.

## Menu focus and account motion

The menu-bar controller opens a borderless nonactivating `NSPanel` and makes that panel key immediately. The first interaction therefore reaches the menu without another focus click. The panel cannot become the main window. Opening it must keep the foreground application unchanged and must not reveal a hidden workspace. Do not replace this boundary with application-wide activation.

`AccountRows` embeds each SwiftUI account row in its own hosting view. AppKit measures the target layout and Core Animation moves and reveals the rendered rows in one transaction. The transparent window canvas grows before expansion and shrinks after collapse. Its top edge stays fixed. The expanded region and the accounts below it move together, without an independent window-height tween or SwiftUI layout on every animation frame.

The row animation requests the current screen's maximum refresh rate. It can retarget from the displayed position; reduced motion applies the destination immediately. This is a scheduling policy, not a guarantee of sustained GPU frame rate. Measure animation cadence and input latency on the target screen under representative load.

See [Accounts and routing](../operations/accounts-and-routing.md) and [Reset Cards](../operations/reset-cards.md) for row actions and independent account details.

## Reading and input

A live call remains bound to its original conversation while the user browses another conversation or starts a new one. The background call bar identifies that owner and supplies Return, Mute and End controls. The other conversation keeps its own editor and draft.

Markdown renders native text, code, lists, and links. Text selection and clipboard operations are read-only. One rendered message shares a selection document across its text blocks. This does not create a selection spanning all messages. Copy controls show short success feedback. Duration and abbreviated token counts remain together at the end of each answer; hovering this row shows detailed usage without changing transcript height.

The history rail follows the reading position. Expanding collaboration exchanges preserves the selected chat and draft. A centered jump-to-latest control represents ongoing work while the user reads older messages. Input controls expose attachments, delivery policy, model/effort, microphone, and live voice without moving execution authority into the UI.

## State and errors

Nonblocking conversation feedback and recovery actions share one band above the composer. A send restriction keeps the normal input bar, locks the editor and centers the shared status inside it. The draft stays saved internally and returns to the editor after recovery; it is not repeated below the error. Errors and quoted passages do not occupy the composer toolbar. Archive status and Unarchive also stay inside the disabled input bar without another notice above it. Parent-managed native chats offer Open parent; Retry is reserved for a failed native connection.

Archive is a normal read-only state. Archived and the short explanation Unarchive to send messages appear inside the disabled input bar, with Unarchive on the right. Read-only notices use a short state followed by one plain sentence about its effect or next action; the explanation wraps when necessary. Background archive-read failures preserve the last confirmed state. A restore failure replaces the ordinary explanation with its actual feedback.

A failed send stays beside its own message in the scrolling transcript, with Not sent and an available Resend action. It does not become a conversation-wide error or a second input panel. App-wide notices go to the notification center; field and operation failures stay beside their owning control.

## Consistent status presentation

GPUI uses the shared `ui_feedback` presentation for account problems and operation notices. Persistent and animated local feedback share a compact inset row, with a quiet tone fill, icon and explicit wording. Text, icons and recovery actions align vertically at the center. Titles and explanations flow together. Login-item and system-sleep recovery buttons stay in the affected feedback row. Account problems appear beside the affected account without requiring a popup. The native menu-bar feedback control keeps its compact icon and explanatory popover.

Place nonblocking conversation feedback and recovery in the band above the composer, field and operation failures beside their control, and app-wide events in the notification center. Keep send restrictions and archive status inside the disabled composer. History retry and summary notices reuse the shared inline feedback component. Use the shared status text and button styles. A whole-page startup failure uses the same status row when no workspace is available. Keep the native menu-bar counterpart compact.

The GPUI notification center orders errors before warnings and information when it selects the bell and badge color. Its status rows use the same feedback component. A notification can be dismissed or cleared from that presentation; this does not remove its underlying source condition. Native feedback popovers close without clearing their source. These are separate interactions.

## UI wording and punctuation

Use short, direct action or status labels. Do not add a status sentence below an account to repeat its warning icon. Use an ellipsis only for an ongoing operation such as `Loading activity…`. Use complete sentences and normal punctuation for explanations in notifications, popovers and hover text.

Keep each label about one user need. Account warning popovers state what happened and the useful next action. Do not append storage labels to an error or join unrelated messages with a bullet. Describe what happened and the next useful action in plain language. Keep internal error variant names and credential-refresh mechanics out of user-facing account explanations.

Use **Fork** for conversation actions in lists, locked conversations and provider recovery. Keep source boundaries, destination providers and no-send explanations beside the action; keep the button label short. Use **Side chat** for an attached conversation and **Side chats** for its list. Native protocol identifiers and Git branch terminology keep their existing names.

Fork, Retry and Unarchive use the same bordered `settings_button` base, including height, padding, radius and interaction feedback. Status text wraps within the conversation rail; an inline title and its explanation flow as one text block. Clicking Fork starts creation after source preparation, with no second confirmation or Cancel control. Conversation progress and recovery use the band above the composer. Creation sends no message.

Reserve `arrow.triangle.branch` for Fork. Account routing uses `point.topleft.down.to.point.bottomright.curvepath` in both GPUI and Menu Bar, with the existing active color and disabled behavior. New local Agent records use `(Fork)` or `(Side chat)` as identity suffixes. Native conversation topics update separately. The source header stays **Main**. Side chat tabs use the current topic, with numbered names until a title is available.

## Verification

The [Assistant lifecycle](../decisions/assistant-lifecycle.md) keeps a stable personal Agent and renews its native context behind the same conversation. Assistant is built in and always available from the sidebar. First use records an idle Agent and its role atomically without a generated user message. It has no Enable control or optional component row. Work retains its role selector, saved teams and shared pending-context feedback. Continuing instructions, teams and duties are managed in chat. The editor and reading state survive a confirmed context transition; older messages retain their native sources. Side chats remain separate native branches.

RAGFlow is an optional component with Install, Update, Uninstall and complete installed version readback. It shares lifecycle buttons, busy state and inline feedback with the other components. Install checks the running Docker-compatible engine and Compose, then prepares RAGFlow, Infinity and the private account. Missing prerequisites use the shared notice; the App does not install or launch a container runtime. Successful installation opens model setup when no models are saved. Expand RAGFlow and select Set up models or Edit to enter one Base URL, an optional API key, an extraction model ID and an embedding model ID. Both models use that shared OpenAI-compatible connection. Save verifies the choices and initializes dedicated Memory without sending chat content. New Memory connections enable automatic learning from completed Assistant dialogue. The Automatic learning switch can stop future learning; existing saved choices remain unchanged on update.

Use GPUI tests in `shell.rs`, `agent_workspace.rs`, `agent_activity.rs`, `agent_markdown.rs`, `agent_archive.rs` and `settings_surface.rs`. Real macOS acceptance must also check focus, typing, scrolling, panel transitions, and transparency in the signed app. A white or missing automation screenshot alone is not evidence that the user sees a blank window.

The Work portfolio fixtures check Project grouping, scoped evidence, source-bound detail loading, global reviewer controls and explicit limits. The collaboration fixtures check cross-chat visibility, expansion without draft changes, participant location and the distinction between recorded messages, tool completion and context references. These checks do not replace signed-app interaction acceptance.

`StatusPanelLifecycleTests` checks first-open and repeated menu focus, foreground application identity, hidden workspace visibility, and fixed-top resizing. `AccountPanelPresentationTests` checks synchronized row movement, clipping, interruption and reduced motion. The GPUI `account_feedback` tests check visible explanations beside the affected account, source revision matching and retention of account state. Its opt-in `DECODEX_MEASURE_ACCOUNT_MOTION=1` presentation sampling separates initial response delay from animation cadence; it does not prove sustained display FPS. `DECODEX_CAPTURE_ACCOUNT_MOTION` selects a screenshot directory for visual review.

See [Conversation presentation and motion](conversation-presentation.md), [Agent coordination](chief-coordination.md) and [Commands and validation](../operations/commands-and-validation.md).

## Retained presentation and drafts

The composer preserves current editable input. Submitted messages retain their own delivery state and exact retry payload; there is no recovered-copy manager. Rich Markdown retains math, Mermaid, weather and source-copy presentation without replacing native execution. The embedded MCP HTML viewer is retired. Ordinary tools and native form prompts remain available. See [conversations](../workflows/conversations-and-recovery.md), [tools](../integrations/tools-plugins-and-apps.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).

The RAGFlow status light distinguishes service installation from Assistant Memory configuration even when collapsed. The saved view aligns the Base URL, model IDs and API key status without showing the key. Edit remains available after Save. The four-field editor places the shared connection above the two model IDs. Empty required values disable Save; explicit submission validates complete input. Editing clears the old editor error. A blank key retains the saved extraction connection key only while its Base URL is unchanged. Collapse retains unsaved input. Cancel discards the draft and error and returns to the saved summary while the component stays expanded. API compatibility, model roles and native-memory consequences remain visible.

Reset Memory first shows an inline confirmation bound to the exact reviewed Memory. Confirming removes only that managed Memory through the native RAGFlow API and initializes an empty replacement with the saved model choices. It retains chat history, model settings, credentials, knowledge selections and service volumes, and preserves the current automatic-learning choice. The replacement starts a new export boundary, so earlier conversations are not replayed. A failed replacement leaves no deleted Memory identity in the private receipt and directs the user to Edit and Save. This is a logical Memory reset, not in-place reindexing or a physical storage-erasure guarantee. The App does not replay old conversations or modify another Memory or an external system.

RAGFlow setup uses the current local service profile and selected Assistant. Profile, runtime or role changes clear its temporary presentation state while preserving the ordinary chat draft. The component's generated credentials and raw scope IDs are not UI fields. Setup progress and failures use shared inline feedback beside the component. See [Runtime architecture](runtime-architecture.md) for process, storage and native-memory ownership.

Component status lights use the existing component owners and the theme’s semantic success, warning, information, error and muted tones. Ready means Codex is ready, EGM is enabled, or Research is connected with its tools enabled. RAGFlow requires both a responding service API and configured Memory for Ready. Warning marks disabled or incomplete setup, information marks operations, error marks failures or an unavailable service, and muted marks unchecked or absent components. Tooltips and accessibility labels state the meaning. RAGFlow checks its public version API in the background on initial inspection and Components entry, with a three-second limit; passive reads retain the previous light without adding loading rows.
