---
type: Reference
title: "Desktop workspace and native glass"
description: "Workspace ownership, native menu focus, compositor motion, and consistent status presentation."
tags: [decodex, architecture, desktop, presentation]
sources:
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
  - id: openwiki-source-ae2ce5cda718ede63f50be9b
    resource: repo://apps/decodex-gpui/src/agent_native_composer.rs
  - id: openwiki-source-e59b6fade5576eca54cd30f7
    resource: repo://apps/decodex-gpui/src/agent_native_menu.rs
  - id: openwiki-source-c6ac7d6b2fdeb458faab2c3a
    resource: repo://apps/decodex-gpui/src/agent_permissions.rs
  - id: openwiki-source-1b7747f8ae67c32e5670817d
    resource: repo://apps/decodex-gpui/src/agent_portfolio_tests.rs
  - id: openwiki-source-6a1c55b61be8ac251a493046
    resource: repo://apps/decodex-gpui/src/agent_portfolio.rs
  - id: openwiki-source-b2084dffd07b4229957a0f94
    resource: repo://apps/decodex-gpui/src/agent_prompt_edit.rs
  - id: openwiki-source-c222de0005be9c363ba17232
    resource: repo://apps/decodex-gpui/src/agent_relation_canvas.rs
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
  - id: openwiki-source-fca3688e96d92c09b7ba015f
    resource: repo://apps/decodex-gpui/src/native_material_panel.rs
  - id: openwiki-source-a2f254020dd8eaabd6366c91
    resource: repo://apps/decodex-gpui/src/session_settings.rs
  - id: openwiki-source-2986b39185cca5c00a29ad1d
    resource: repo://apps/decodex-gpui/src/shell_status.rs
  - id: openwiki-source-1291f5243fa6c9cb52149bda
    resource: repo://apps/decodex-gpui/src/shell.rs
  - id: openwiki-source-2f256c95c226c1326d164ad3
    resource: repo://apps/decodex-gpui/src/ui_feedback.rs
  - id: openwiki-source-546812ff3637690f4a184b89
    resource: repo://apps/decodex-gpui/src/ui_preferences.rs
  - id: openwiki-source-c29d70b5569f29b7122f2f6b
    resource: repo://crates/decodex-runtime/src/native_agents.rs
  - id: openwiki-source-6085af9603cb7ac0ff3ef1cf
    resource: repo://crates/decodex-runtime/src/work_portfolio.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
  - id: openwiki-source-c0eb738b42368debe7ef7545
    resource: repo://database/src/works/commands.rs
generated: { by: "codex", at: "2026-10-30T15:17:49.972Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-30T15:17:49.972Z
---

# Desktop workspace and native glass

## Information hierarchy

The Agent conversation is the primary workspace. The left sidebar selects the overview and projects. The right Collaboration panel keeps recorded exchanges and pending response requests visible across chats. The bottom Dock presents Agent relationships through its graph or list.

Each independent Agent owns its conversation and composer. Messages go to that conversation, including when another Agent appears first in the overview. Assistant and Project Lead responsibilities do not redirect the input. Native approval controls stay in the selected conversation and retain the exact pending request.

Collaboration groups source-backed exchanges by participant pair and expands their recorded text without switching the active chat or draft. A participant control locates that Agent in the graph. Tool completion alone is not delivery, and a context reference is not a message. Missing reply text remains explicit. Pending user requests retain their existing native controls; this panel does not answer them automatically or create a second question store.

Global shortcuts in `shell.rs` are Command-E for the left sidebar, Command-B for the inspector/agent side, and Command-J for the graph. Window controls stay in the global shell. Settings is a separate presentation with bounded scrolling and shared spacing tokens.

The Graph groups Agents by recorded Project membership. A dashed rounded boundary separates that scope from solid relationship links. Selecting an Agent highlights its Project boundary. Tree, Radial and Grouped retain compact branches within a Project and clearance between Projects. Point overview retains boundaries while hiding small labels. These frames do not move an Agent to another Project or change ownership.

Settings > Agents supplies permission and speed defaults for new independent conversations. The input-bar shield selects the current Agent's native permission mode; speed stays inside the model menu. Existing conversations retain their own settings, and native Subagents follow native inheritance rather than a Decodex broadcast.

Shift-Esc toggles Graph or conversation fullscreen according to the active pane. Pane capture tracks clicks across cached Graph layers, and the native composer forwards actions to the persistent shell. Graph fullscreen hides both sidebars and restores their saved visibility and width preferences on exit.

The conversation header groups each source with its open Side chats. Use **+** to review and create another native attached branch. Switching tabs preserves each conversation's draft and reading position; closing a tab retains the native branch. See [Side chat tabs](conversation-presentation.md#side-chat-tabs).

## Project Work and Guardian

Work and Guardian are entries in the existing sidebar. Work groups outcomes by Project and can show the same accountable Agent on several rows. Assistant and Project Lead selectors choose existing Agents; they do not create role-specific conversations. The existing conversation, Collaboration panel and Dock remain available.

Expand a Work to read its current requirements, authoritative source, completion criteria, hold reason and resumption condition. Materials open their authorized references. Results show their exact native item or artifact digest. Collaboration Requests show their recipient, issued requirements, scoped Materials, delivery, acceptance, return and assessment. Discuss with owner opens that Agent's conversation. Awaiting Assessment is a derived activity label, not another Work state.

The overview carries headers; selected Work evidence loads separately. Reads are bounded to 100 records per category and disclose additional retained evidence. Large selected records use digest-checked 8 KiB pages with a 32 MiB ceiling. The UI labels cached evidence and offers refresh when the source changed. A missing or stale read does not establish completion.

Pause Work prevents new Decodex contributions and completion. It cannot roll back already-running actions and does not replace native interruption. Work association with a Project does not change the native conversation's working directory or execution permission.

Guardian shows review records across Agents and Projects. Its global **Save review details** switch controls optional bounded observation, not conversation permissions. The selected runtime does not provide exact action Review IDs for private reviewer records, so the UI displays Action association unavailable. It does not infer a link to the latest action. Turning details off stops new records; it does not approve requests or erase retained history.

Only native assistant output and item-free terminal metadata enter retained private records. Raw reasoning frames are excluded. The store retains at most 128 records of 64 KiB each; the snapshot previews the newest 32 and loads the exact selected record on demand. This is not a complete reviewer transcript or proof of a live approval decision.

## List status filters

The existing conversation browser and Work list each offer status filters and **All statuses**. Each view keeps its own selection through the host-local presentation preference owner. Status filtering combines with the existing Project and text search filters. A status that no longer has a matching row remains selected and visible until the user clears it.

Managed conversation labels come from the existing status projection, native child rows keep their reported labels, and Work uses its current business state. Filtering does not change those facts, send a command, move a conversation or clear a draft. Work remains grouped by Project, and exact row opening and selected evidence keep their existing owners. See [the list workflow](../workflows/conversations-and-recovery.md#filter-existing-lists-by-status) and [qualification limits](../testing/upstream-acceptance-boundaries.md#list-status-filter-qualification).

## Read-only evidence views

Agent preferences includes **Memory summary** for the current native root conversation. It reads the selected native summary without starting or resuming a conversation. Refresh and Copy summary are explicit actions. Empty native memory can mean disabled, missing, unreadable or blank memory. A source or binding change removes the old view. The desktop does not edit, regenerate or interpret the summary as instructions.

**Actual usage** is available in Agent preferences and expanded Work details. The panel shows known whole-turn tokens, recorded duration, coverage counts and bounded exact source examples. Unknown counters stay unknown. Work uses saved turn references across retained requirements; linked turns can contain other activity, and known shared turns are excluded from linked-turn totals. Recorded durations can overlap. Copy usage evidence exports this observed view after an explicit action.

The existing Collaboration panel includes retained question answers and independent contribution cycles. Exact reply delivery and receiving-turn outcome remain distinct from question resolution or Work assessment. Repair cycles preserve their own sources. See [Agent coordination](chief-coordination.md#read-only-collaboration-evidence-and-usage) for the joins and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md#selected-read-only-views) for verification limits.

Agent preferences also includes **Attached branches**. It lists the newest 100 saved Decodex branch receipts for the exact source conversation. **Open branch** uses the existing conversation navigation and preserves the source draft. An uncertain creation stays unconfirmed; the list does not repeat it. An archived branch must be restored in Codex before refresh can open it. See [conversation recovery](../workflows/conversations-and-recovery.md) for creation and recovery.

## Material and focus ownership

`window_material.rs` selects Regular or Clear material. macOS uses native Liquid Glass when supported; the fallback requests GPUI platform blur. Actual transparency depends on platform compositor support. Reduced-transparency preferences and `DECODEX_DISABLE_LIQUID_GLASS` prevent the small native-glass panels.

`native_material_panel.rs` bridges composition only. GPUI still owns UI state, layout and event handlers. The composer is an attached child window created by the main workspace, not another application or service. Settings must not create one. The workspace remains the main window while an attached control can own keyboard focus. See [conversation presentation](conversation-presentation.md#floating-controls-and-motion) for frame ownership and animation policy.

Archived and blocked conversations keep the normal composer geometry with sending disabled and the draft retained. A locked conversation with a readable, currently bound native prompt offers **Continue in a new branch…**. This opens a reviewed native branch without taking over the original writer. Certain detail/full-screen states omit the composer. Reopening includes a short layout-settling interval; failures fall back to GPUI presentation. Notification placement and opacity belong to the same native-panel composition boundary.

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

Conversation errors and recovery actions share one band above the input bar. The input is disabled, its draft is retained, and quote placeholders stay hidden. Parent-managed native chats offer Open parent conversation; Retry is reserved for a failed native connection.

Archive is a normal read-only state. Archived and the short explanation Unarchive to send messages appear inside the disabled input bar, with Unarchive on the right. Read-only notices use a short state followed by one plain sentence about its effect or next action; the explanation wraps when necessary. Background archive-read failures preserve the last confirmed state. A restore failure replaces the ordinary explanation with its actual feedback.

A failed send stays beside its own message in the scrolling transcript, with Not sent and an available Resend action. It does not become a conversation-wide error or a second input panel. App-wide notices go to the notification center; field and operation failures stay beside their owning control.

## Consistent status presentation

GPUI uses the shared `ui_feedback` presentation for account problems and operation notices. Status rows use severity color with an icon and explicit wording. Account problems appear beside the affected account without requiring a popup. The native menu-bar feedback control keeps its compact icon and explanatory popover.

The GPUI notification center orders errors before warnings and information when it selects the bell and badge color. Its status rows use the same feedback component. A notification can be dismissed or cleared from that presentation; this does not remove its underlying source condition. Native feedback popovers close without clearing their source. These are separate interactions.

## UI wording and punctuation

Use short, direct action or status labels. Do not add a status sentence below an account to repeat its warning icon. Use an ellipsis only for an ongoing operation such as `Loading activity…`. Use complete sentences and normal punctuation for explanations in notifications, popovers and hover text.

Keep each label about one user need. Account warning popovers state what happened and the useful next action. Do not append storage labels to an error or join unrelated messages with a bullet. Describe what happened and the next useful action in plain language. Keep internal error variant names and credential-refresh mechanics out of user-facing account explanations.

## Verification

Use GPUI tests in `shell.rs`, `agent_workspace.rs`, `agent_activity.rs`, `agent_markdown.rs`, `agent_archive.rs` and `settings_surface.rs`. Real macOS acceptance must also check focus, typing, scrolling, panel transitions, and transparency in the signed app. A white or missing automation screenshot alone is not evidence that the user sees a blank window.

The Work portfolio fixtures check Project grouping, scoped evidence, source-bound detail loading, global reviewer controls and explicit limits. The collaboration fixtures check cross-chat visibility, expansion without draft changes, participant location and the distinction between recorded messages, tool completion and context references. These checks do not replace signed-app interaction acceptance.

`StatusPanelLifecycleTests` checks first-open and repeated menu focus, foreground application identity, hidden workspace visibility, and fixed-top resizing. `AccountPanelPresentationTests` checks synchronized row movement, clipping, interruption and reduced motion. The GPUI `account_feedback` tests check visible explanations beside the affected account, source revision matching and retention of account state. Its opt-in `DECODEX_MEASURE_ACCOUNT_MOTION=1` presentation sampling separates initial response delay from animation cadence; it does not prove sustained display FPS. `DECODEX_CAPTURE_ACCOUNT_MOTION` selects a screenshot directory for visual review.

See [Conversation presentation and motion](conversation-presentation.md), [Agent coordination](chief-coordination.md) and [Commands and validation](../operations/commands-and-validation.md).

## Retained presentation and drafts

The composer preserves current editable input. Submitted messages retain their own delivery state and exact retry payload; there is no recovered-copy manager. Rich Markdown retains math, Mermaid, weather and source-copy presentation without replacing native execution. The embedded MCP HTML viewer is retired. Ordinary tools and native form prompts remain available. See [conversations](../workflows/conversations-and-recovery.md), [tools](../integrations/tools-plugins-and-apps.md) and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).
