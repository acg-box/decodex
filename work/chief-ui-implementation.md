# Chief workspace implementation

## Scope

Implement the accepted conversation-first GPUI design on source revision
579b8d74f in /Users/x/code/acg-box/decodex. Preserve native blurred windows,
material tokens, account behavior, and existing service authority.

## Delivery slices

1. Chief is the default. Remove the parallel direct-chat product tab. Keep
   historical conversations reachable. Open work pages explicitly, deduplicate
   tabs, preserve Chief drafts and conversation position, and return on close.
2. Compact Chief transcript without avatars, repeated names, or execution tables.
   Worker pages expose existing history and exact pending requests. Keep explicit
   message recipient behavior; this protocol only sends user text to Chief.
3. Derived dependency graph with pan, zoom, scope navigation, selection and work
   entry. Use only service identities. No invented Factory/project records.
4. Timeline of observed events. The protocol does not expose execution intervals;
   never use record creation/update timestamps as run duration.
5. Focused native tests, formatting, build and visual capture. Inspect the result.

## Architecture and algorithm

ChiefSurface owns work-page selection, drafts and protocol requests. A child
workspace module owns presentation and a pure graph projection. Native GPUI
canvas/PathBuilder owns geometry. The graph is bounded to 100 work records by the
protocol. Use deterministic Kahn layering over current dependency edges, with a
stable fallback row for cycles. std collections suffice; no new layout/runtime
library is needed. Existing retired Program layout has the wrong data ownership.
Validate branches, missing IDs and cycles. Reconsider a layout library when graph
size or interactive edge editing exceeds this bounded projection.

## Validation record

Pending. No live capability or execution duration is inferred from fixtures.

## Delivered first version

- Chief is the default desktop destination. History is no longer a global navigation destination. The redundant product-purpose banner and parallel direct-chat tab
  are removed.
- Open work tabs use stable work IDs. Reopening reuses a tab. Closing the active
  work tab returns to Chief. Drafts, transcript scroll handles and graph view state
  remain local to their owners. Switching service profiles clears cached pages.
- Sidebar lists real goals and items that need user attention. No synthetic
  Project or Factory inventory is displayed.
- Chief conversation has no avatars or repeated speaker names. Worker pages expose
  saved execution messages, exact requests and optional metadata. Follow-up returns
  explicitly to the Chief composer; no user text is silently sent to another role.
- Graph renders exact dependency edges with deterministic layers and inherited
  branch lanes. Drag/scroll pans; Control/Command-scroll and +/- zoom. Enter opens
  work; double-click enters a parent or opens a leaf. Expand/Restore keeps full
  canvas exploration explicit. Cycles remain visible and are flagged.
- Timeline renders saved receipt timestamps for the selected conversation. It
  does not claim unavailable per-worker execution intervals or live duration bars.
- Existing native blurred-window configuration and shared translucent material
  tokens remain unchanged. The offscreen captures cannot prove desktop blur.

## Fresh verification

- `cargo +stable test -p decodex-gpui --all-targets --features visual-capture`:
  270 passed, 6 existing opt-in tests skipped, 0 failed across both targets.
- Package Clippy with all repository deny flags: passed.
- `cargo make fmt-rust-check`: passed. The repository owns its pinned formatter;
  build/test/compiler commands use stable.
- `python3 -m unittest tests/scripts/test_vnext_architecture.py`: 16 passed.
- `git diff --check`: passed.
- Native capture scenarios inspected: `target/visual-tests/chief-workspace-final.png`
  and `target/visual-tests/chief-worker-final.png`. These use explicit capture-only
  fixtures, not live product records.
- Canonical `scripts/macos/stage_decodex_app.sh` completed in the task-specific
  `target/chief-workspace-app` directory. Bundle, helpers and libraries passed
  signing verification. No installed application was replaced.
- The staged application process launched. Desktop inspection returned
  `cgWindowNotFound`, so visible live-window and desktop-blur acceptance remain
  unverified. Offscreen GPUI rendering is verified separately.

## Remaining protocol boundaries

Direct user messages to a Worker, rich provider diff/tool streams and historical
execution intervals are not available through the current Chief protocol. This
version exposes existing worker records and routes follow-up explicitly to Chief.
Multi-Factory and Project ownership remain product direction, not invented UI data.


## UI refinement after desktop review

- Desktop capture exposed stacked sidebar tint: shell + content + sidebar produced
  an almost opaque region. Chief sidebar and graph now sit directly on the shell,
  with one dedicated light tint. The composite sidebar opacity is about 0.40;
  a material regression assertion checks this budget.
- Remove global History navigation and its keyboard shortcut. Command-1 selects
  Chief. Existing persisted conversation data is not deleted.
- Panel controls are in the window title bar: left sidebar on the left; graph and
  timeline on the right. Use functional 16px line icons with tooltips and accurate
  state. Shortcuts are Command-B, Command-Shift-B and Command-J.
- Sidebar state is global to the Chief workspace. Graph/timeline visibility and
  graph viewport state follow each open work page. Title-bar state observes the
  current Chief surface. No panel toggle row remains in the conversation body.
- Empty workspace shows a centered composer. Hide the empty graph, zero attention
  count, empty goal section and single-page tab strip. Disable graph/timeline
  controls until there is corresponding data. Normal snapshot refresh does not
  flash a status banner over retained data.
- Fresh package tests: 272 passed, 6 existing opt-in tests skipped. Strict Clippy,
  formatter checks and task-specific signed staging passed. The exact staged
  process is the only running Decodex GUI instance.
- Live accessibility readback confirms History is absent, controls are in the
  global toolbar, and toggling the sidebar removes its content. Initial live
  capture confirms the sidebar is lighter than the conversation surface. Later
  desktop screenshots returned blank despite responsive accessibility; user
  confirmation is pending to distinguish capture failure from a visible problem.

## Native symbols and settings organization

- User confirmed the application was not blank. The later white desktop images
  were a capture-tool issue; native accessibility remained responsive.
- Replace hand-built toolbar paths with macOS SF Symbols. The repository-owned
  Swift generator renders regular-weight 14-point symbols at 3x. GPUI embeds them
  in the executable, so staging does not rely on source-tree asset paths.
- Accounts and Diagnostics now share the Settings container. Diagnostics is a
  secondary settings section. The global toolbar has one gear entry and supports
  Command-comma. Chief remains directly reachable with Command-1.
- Remove implementation-oriented settings prose. Use compact controls and a
  bounded general-preferences column. Keep real error and unavailable states.
- Fresh validation: 272 package tests passed, 6 existing opt-in tests skipped;
  strict Clippy, formatting and signed app staging passed. The running preview
  is updated in place, without opening an old installation alongside it.

## Balanced sidebar and reduced title-bar branding

- Increase the direct Chief sidebar tint from 14% to 36%; with the shell, its
  composite opacity is about 55%. Keep native background blur and maintain a
  lighter sidebar than the conversation surface.
- Remove the in-window Decodex wordmark and logo. Keep native window controls,
  panel controls, and Settings. Remove the unused title-bar image resolver;
  packaged Dock and menu-bar assets remain unchanged.
- Retry snapshot reads after transient startup or stale-state failures when a
  profile exists. Avoid overlapping reads and repeated reads for ready idle
  workspaces. The retry does not repeat a message or execution request.
- Regression coverage checks startup recovery, no-profile and loading guards,
  and idle versus active polling. Fresh package results: 274 passed, 6 existing
  opt-in tests skipped. Strict Clippy and formatter checks passed.
- Signed staging passed. Live readback confirms the startup Refresh error clears
  automatically, the title-bar branding is absent, and sidebar toggling works.
  A live screenshot shows the updated sidebar tint. Only the task preview GUI
  process is running.

## Icon alignment and interruptible interaction motion

- Center the visible SF Symbol ink within a 16-point, non-shrinking image box.
  Icon-only workspace actions use a centered 28-point hit area. Close, back,
  zoom and send actions now use the same native symbol source as panel controls.
- Add a shared cubic ease-out transition with a 200 ms duration. New targets
  start at the current interpolated value, including direction changes. Frames
  are requested only while values are changing; no idle animation timer runs.
- Animate sidebar width, graph width and expansion, timeline height, graph zoom,
  settings switch travel, and button feedback for mouse and keyboard input.
- Measure disclosure content before animating Chief setup, work metadata and
  earlier history. Keep content mounted through its closing transition, then
  remove it to prevent hidden controls from remaining interactive.
- Add a regression test for continuity during reversal and termination at rest.
  Fresh package validation: 276 passed, 6 existing opt-in tests skipped; strict
  Clippy and formatting passed. Native UI checks covered repeated sidebar
  toggles and setup expansion/collapse. Fixture captures cover the graph,
  timeline and centered panel controls without starting real worker tasks.

## Conversation entry review against the user screenshot and Zeron reference

Reference: https://x.com/winglee/status/2099558809727631595
Source inspection: https://github.com/zeronsh/zeron/tree/main/crates/ui/src

1. Entry screen: the supplied screenshot showed readable background video and
   a setup form competing with the conversation. Remove the greeting and setup
   controls from the entry; keep one bounded composer and a collapsed sidebar.
2. Optional preferences: move existing Chief defaults to General Settings under
   a collapsed Advanced disclosure. Preserve existing model selection, routing,
   working-directory values and sandbox behavior. No execution permission is
   expanded, and no provider request is sent by this UI change.
3. Materials: use the supported AppKit Sidebar material with explicit
   BehindWindow blending on GPUI's existing visual-effect view. Bound background
   transmission in the conversation and sidebar to keep bright content behind
   the window from competing with text. Preserve blur and translucent chrome.
4. Visual hierarchy: remove redundant entry copy, use a quieter composer edge
   and shadow, and group the right-side title-bar controls. Preserve animation.

The Zeron reference has dedicated background-image, effect and alpha-mask code;
its controlled artwork is not evidence that arbitrary underlying windows should
remain readable. No artwork or implementation was copied into this repository.
Fresh checks: 276 tests passed, 6 opt-in skips; strict Clippy passed.
- Signed staging and live verification passed. The entry exposes only the Chief
  message field and Send; the Advanced controls expand only within Settings.
  No values were changed during the check. Only one preview GUI is running.
- The browser URL policy rejected a local HTML backdrop test. No bypass was
  attempted. Native screenshots on the existing desktop were checked, but a
  controlled light/dark backdrop comparison remains unverified.

## Floating window controls

- Remove the full-width painted title bar and its horizontal separator. Render
  compact absolute-positioned control groups above full-height content.
- The left group reserves space for native traffic lights and contains the
  sidebar control or the return-to-Chief action. The right group contains
  available work panels, connection state and Settings. Hide empty panel actions.
- Pane-owned top clearance prevents tabs, sidebar rows and settings content from
  crossing the controls. Background materials continue behind the controls.
- Preserve the native window controls, drag/double-click behavior, shortcuts,
  and existing button/panel motion. Fresh tests: 276 passed, 6 opt-in skips;
  strict Clippy passed. Offscreen capture confirms the full-width bar is absent.
- Signed staging passed. Live native screenshots confirm the unbroken content
  surface and small corner groups. Sidebar and Settings controls respond, and
  Settings content clears the floating controls. The app is left on Chief with
  the sidebar collapsed. Only one preview GUI is running.

## Alignment and glass correction

- Review the live entry and GPUI Kit's rendered button examples. Candidate:
  https://github.com/longbridge/gpui-kit (main observed 2026-09-15).
  Its workspace uses gpui-pre 0.3.5; this repository uses Zed GPUI revision
  92f315647f776854053fc334b73110d97964bc5f. This is not a drop-in component swap.
  risk_coverage: manifest compatibility and component-source/preview inspection;
  no transitive security review. risk_delta: no dependency changes. decision:
  defer adoption to an explicit GPUI migration; retain the current framework.
- Share control group geometry rather than duplicating its padding and radius.
  Center native traffic lights from their actual button height against the same
  group center. Align input text and Send on one centerline; reduce excess input
  height. Hide the normal online indicator while retaining connection warnings.
- Restore bounded glass transmission: about 66% composite conversation tint and
  57% sidebar tint, versus the previous 94% and 83%. Keep the native blur layer.
- Fresh validation: 276 package tests passed, 6 opt-in skips; strict Clippy passed.
- The live empty-state screenshot showed the restored backdrop response and
  aligned input/Send centerline. A repeated native capture returned the known
  white capture failure while accessibility remained responsive. Temporary
  input was verified and removed without submission. A fresh deterministic
  rendered draft capture confirms the text/Send alignment; it does not prove
  desktop blur. Reduce the overly heavy composer shadow after live review.

## Compact controls and back/forward navigation

- Reduce floating group height to 28 points and its radius to 8. Chrome controls
  use 24-point slots; the native traffic-light centers follow the group height.
  Increase panel-symbol rendering to 20 points to balance their visible ink
  against native buttons. Keep the composer Send control at its existing size.
- Add back/forward controls and Command-[ / Command-] actions. History captures
  Chief, specific workers and Settings destinations, deduplicates readbacks,
  discards forward entries after a new visit, and retains at most 100 locations.
  Traversal restores cached work-page view state and does not replay commands.
  Skip work records that are no longer available. Boundary buttons do nothing
  and advertise their unavailable state without entering keyboard tab order.
- Add navigation branch/deduplication and native keyboard tests for round trips
  through Chief, Worker and Settings. Fresh checks: 280 passed, 6 opt-in skips;
  strict Clippy passed.
- Signed staging passed. Live UI traversal verified Chief -> Settings ->
  Diagnostics -> Back to Settings -> Back to Chief -> Forward to Settings.
  Boundary labels changed correctly. The preview is left on Chief with forward
  history available. Only one preview GUI is running.

## Settings icon state cleanup

- Remove the right control group's painted container. The settings icon now has
  one 28-point hit area and one subtle selected/hover surface, without nested
  selected borders. Retain a keyboard-only focus border and animated feedback.
- Stop pointer-down propagation at Settings so interaction cannot initiate a
  window drag. Existing destination and shortcut actions remain unchanged.
- Strict Clippy passed. This scoped style adjustment adds no behavior-mirroring
  tests; verify the selected state in the running native app.
- Signed staging and format checks passed. Live screenshots verified both
  mouse-selected and keyboard-focused Settings states. Mouse selection has no
  nested border; Shift-Tab gives the gear one subtle focus outline. The app is
  left on Settings in pointer mode for inspection, with one GUI instance.

## Compact composer and interaction feedback

- Reduce the Chief composer from 66 to 46 logical pixels. Use 6-pixel vertical
  padding, a 32-pixel input, and a 10-pixel corner radius. Keep Send at 28 pixels.
- Add a 1.5-pixel press and release motion to shared controls. Route changes use
  a 200-ms arrival transition; unchanged routes do not restart it.
- Correct the transition wrapper to preserve the full-height flex layout after
  visual capture exposed collapsed content. Re-capture confirms centered content.
- Validation: 280 tests passed, 6 opt-in tests skipped; strict Clippy passed after
  the layout correction. Signed preview rebuilt. Native UI checks cover Settings,
  advanced disclosure, and Back to Chief. Static screenshots verify geometry;
  they do not measure perceived animation smoothness.

## Native control alignment after window changes

- Replace the one-time hidden-window traffic-light measurement with a post-frame
  measurement on initial display, activation, bounds changes, and appearance
  changes. Read the current native button height for the shared control center.
- Strict Clippy, formatting, and signed staging passed. Relaunched one preview.
  Native activation and Raise were exercised. The capture privacy indicator covers
  traffic lights in screenshots, so direct pixel verification of those circles
  remains unavailable in this capture session.

## Global status surface

- Move Chief load and submission feedback plus the shell connection indicator to
  one bottom-right status entry. Expand a compact card for details and read-only
  refresh or Diagnostics actions. Do not replay commands from recovery controls.
- Keep failed refresh status visible while automatic refresh runs. Clear it on
  successful recovery. Add coverage for status retention and retry suppression.
- Remove the inline warning banner and composer feedback row. Keep native control
  alignment observers from the same delivery.
- Validation: 280 tests passed, 6 skipped. Strict Clippy and formatting passed.
  Visual capture: target/visual-tests/chief-status.png. Native screenshots show
  aligned traffic lights and adjacent icons. Explicitly size the status container
  to the card width so expanded content shares its parent hit region.

## Conversation behavior audit and repair

- Continue idle snapshot polling so external or delayed service work reaches UI.
- Preserve same-work saved history across transient read failures.
- Add Sending, Starting, Working and uncertain execution feedback plus an exact
  turn Stop action to the primary conversation surface.
- 280 GPUI tests passed; 6 skipped. Strict Clippy, formatting, and signed staging
  passed. Conversation and running-worker captures were inspected.
- Live disposable chief_service_smoke with gpt-6-astra and reconnect scope passed:
  Chief response/history, two independent worker results, Chief result disposition,
  timer-only reconnect to the same account, zero send commands during recovery,
  zero pending host errors, and clean service shutdown.
- This is not evidence for token streaming or recursive Chiefs. See
  work/chief-functional-audit.md for the remaining product boundary.

## Functional completion — protocol 2.17, schema 17

- Complete the five gaps listed in the earlier audit: current-turn streaming,
  typed questions and approvals, saved-history pagination, recursive managers,
  and project-directory workspaces. See chief-functional-audit.md for boundaries.
- Keep conversation drafts with their recipient. Preserve accepted first-input
  clearing when snapshot polling discovers a new Chief before acceptance returns.
- Upgrade old manager provider tools once, retaining Decodex work identity and
  saved history. Do not replay old input or silently retry uncertain creation.
- Use atomic transcript and organization snapshots. Deliver child results through
  the nearest manager boundary and recover unbound durable child work.
- Move saved system events out of chat bubbles into the global status card. Show
  an active warning only for pending errors; label historical information as the
  last saved service event. Keep it scoped to the selected conversation.
- Validation: 690 Rust tests passed (7 opt-in/visual tests skipped), 16 architecture
  tests passed, strict Clippy, formatting, and diff checks passed. Signed macOS
  staging passed. Two isolated live hierarchy qualifications passed, including
  partial output and provider reconnect without send replay.
- Inspected final question, command-approval, and scoped hierarchy captures. The
  hierarchy fixture has no manager transcript; it checks graph and composer layout.
  Native preview inspection confirmed that the user's saved messages survived the
  additive upgrade and that the floating native controls remain aligned.

- Native acceptance found a stale connection alert after the service had recovered.
  Add transactional connection-error deduplication and a recovery receipt after
  binding verification. The receipt changes only connection events. Add coverage
  for failure, repeated probe, successful recovery, and a later new failure.

- Final native/service readback: exactly one preview process; original Chief and
  provider thread preserved; pending connection events changed from one to zero
  after successful reconnection. The native conversation shows saved messages
  without the inline service error, and the status entry is normal again.

## Native message text and visible turn boundaries

- Send each pending user message as its exact text in a native Codex turn/start
  input. Do not embed inbox metadata in the user's message. Prioritize one fresh
  user input per turn; leave other user inputs and background evidence unclaimed
  for their own dispatch. Keep durable delivery receipts in SQLite.
- Background wakes contain only evidence identity, work identity, event kind, and
  parsed payload. Do not repeat source IDs, receipt times, or delivery flags in the
  model input. Keep the evidence boundary explicit.
- Increase transcript row spacing to 28 logical pixels. Add 24-pixel bottom padding
  and a quiet divider to each completed assistant response. Keep paragraphs inside
  a response together without avatars or repeated speaker headings.
- Runtime: 295 tests passed, one opt-in test skipped. Tests assert exact outgoing
  user text, preserved line breaks, separate queued turns, and background evidence
  remaining unclaimed while user messages dispatch. Strict Clippy and formatting
  passed. Inspected chief-message-spacing.png before native staging.
- Existing provider conversation records are retained. The clean input format
  applies to new turns; it does not rewrite previously sent Inbox messages.

## Project navigation and interactive work demonstration

- Replace the role/status sidebar with Overview and Projects. Show only workspace
  managers and top-level executable managers as project entries. Pending decisions
  appear as project counts; open the first waiting work item from that count.
  New project prepares a natural-language request in the main Chief composer.
- Preserve per-page panels. New real project pages do not force the graph open.
- Replace message-only timeline ticks with named, clickable latest work-update
  cards when the selected scope has tasks. These are last-observed update times,
  not invented execution durations. The history-only fallback remains for chats.
- Add Explore a demo in the sidebar. It creates a separate in-memory GPUI surface
  without a service profile. Parent commands are blocked while the demo is open.
  Exit drops the demo and reveals the unchanged real surface and draft.
- The five-stage simulation shows two parallel workers, a release decision that
  stops playback, dependent verification, and a final release summary. Play/Pause,
  approval, Reset, graph selection/drill-down, and timeline navigation are available.
  All content is explicitly simulated; no provider threads or files are created.
- Validation: 286 GPUI tests passed, six opt-in/visual cases skipped. Added isolation,
  decision-gate, completion, and worker navigation coverage. Strict Clippy and
  formatting passed. Inspected target/visual-tests/chief-demo.png.

- Native acceptance exercised Play, pause at the decision gate, explicit demo
  approval, dependent verification, completion, and timeline-to-worker navigation.
  Reset the preview to stage 1 for user exploration. Kept one preview process and
  confirmed that the real Chief still has zero pending events and the same thread.

## Real supervision flow replaces the demonstration

The user rejected simulated execution. This supersedes the interactive demo above.

- Remove the in-app demo surface and its Play, Pause, Reset, and simulated approval.
  Keep visual-test fixtures behind their existing test/capture configuration.
- Keep Overview as the real Chief conversation. Clicking a graph node opens its
  real conversation directly; remove Open work. Manager nodes open their own scope.
- Preserve visible panels when entering a new conversation. Existing page state
  remains restorable. The status read action is named Refresh, not execution retry.
- Build Activity from durable assignment timestamps and saved per-work user inputs,
  reports, and system events. Keep event history separate from current graph state.
  Refresh visible-scope history every five seconds, reading the most recently
  updated 24 work items, up to one bounded history page each. Display the latest
  50 events; this is a recent-events view, not a complete audit export or durations.
- The real example is a read-only Decodex product-flow review performed by the
  existing personal Chief, a project Chief, and workers. Keep real conversations
  and results accessible; do not replace them with fixture text.

### Real review evidence and repairs

- The existing Chief accepted command real-product-review-20260916-1. Created the
  Decodex workspace manager and two actual source-review workers. The workers read
  repository files, returned reports, and the manager reviewed and disposed both
  results before the root Chief accepted the project report. No pending events
  remained. These are retained real work records, not disposable test fixtures.
- Workspace: decodex-product-review-20260916. Worker identities:
  decodex-review-a-gpui-20260916 and decodex-review-b-chief-20260916.
- Add the owning manager to the graph. Blue connections show durable reporting
  relationships; gray arrows remain prerequisite relations. Single-click opens
  the corresponding conversation, including a manager's subordinate scope.
- Repair issues identified by the real review: keep drafts when New project is
  clicked; allow activity with assigned work and no conversation history; reopen
  acceptance state on manager redispatch; save manager instructions atomically
  with dispatch claims and bind them to acknowledged turns; prioritize final
  answers within the bounded result budget while retaining chronological display.
- Instruction events never trigger a new manager wake. Ambiguous dispatch keeps
  the original claim and does not replay it. A delivery receipt is not work acceptance.
- Regression coverage checks manager acceptance invalidation, instruction identity,
  duplicate-claim rejection, final-answer retention, and actual graph ownership.

- Final validation: 39 database tests, 296 runtime tests, and 284 GPUI tests passed.
  Strict Clippy, formatting, signed staging, and diff checks passed. Native readback
  after relaunch retained the same four real work identities, all child work
  resolved, and zero pending events. Visually inspected the real project manager
  above its two workers with reporting links, and saved assignment/report cards.

## Conversation readability and chronological timeline

- Render saved replies and live output as native GPUI Markdown. Support headings,
  emphasis, lists, tables, quotes, code blocks, inline code, and supported links.
  Keep remote image loading and code syntax highlighting outside this change.
- Place user messages in a constrained right-aligned bubble. Keep assistant replies
  left-aligned with spacing between messages. Do not add avatars or repeated names.
- Replace horizontal activity cards with a vertical connected timeline. Show UTC
  dates and times, assignment and reporting direction, and plain-text previews.
  Clicking a record opens its real conversation. Preserve repeated reports and
  reading position. The existing recent-history bounds still apply.
- Remove internal worker disposition notes from displayed assistant replies.
  Preserve the stored records. Use readable worker labels when titles are raw IDs.
- Inspect the native Markdown capture and the running real Chief conversation,
  including a table, the graph, and the connected timeline. Keep test fixtures
  restricted to the visual-capture entry point. Stage and open one signed preview.
- GPUI suite passed before the added timeline regression; the new regression passed
  in both binary targets. Runtime unit and integration suites passed after fixing
  a stale future-protocol test to derive the next minor version from CURRENT_VERSION.
  Final strict Clippy, repository formatting, and diff checks passed.
- The personal Chief thread 01a0ab10-9668-7cf0-bda4-39d9769a5537 uses /Users/x.
  Its projectless placement in Codex Chats is consistent with that working directory;
  the placement is not an automatic classification of the Chief role.
- Runtime limitation at verification: the retained real work and reports are readable,
  but Chief process restoration reports ProcessUnavailable. The status remains
  visible; no successful new model turn is claimed by this UI verification.

## Reading layout and provider usage

- Right-align user bubbles with a neutral translucent fill, no border, no repeated
  sender label, and a maximum width of 78 percent. Give assistant Markdown the
  available conversation width with no bubble or fixed 840-pixel column limit.
- Show provider-reported duration after each saved reply when available. Show input
  and output token totals for the current provider thread below the composer. Input
  includes cached tokens; output includes reasoning. Context uses the provider's
  latest `last.totalTokens` and `modelContextWindow`, not cumulative session totals.
- Verify the source fields against JSON Schema generated by the installed Codex
  app-server. Consume `thread/tokenUsage/updated`; reject negative or missing
  required counts. Do not fabricate historical counts or infer model capacities.
- Migration 18 saves bounded usage for the exact current provider thread and active
  turn. Usage does not create inbox events or wake a manager. Readback excludes
  previous provider-thread revisions. Protocol 2.18 carries the optional facts.
- The regression covers stale-turn rejection, malformed counts, persistence after
  completion and restart, and absence of manager wake events. Duration projection
  keeps unknown values absent. Native capture `target/visual-tests/chief-reading-metrics.png`
  confirms bubble alignment, full-width tables, duration, and the usage line. Capture
  values are isolated fixtures; the real conversation continues to use real records.
- Validation passed: database and GPUI suites, protocol and runtime suites, strict
  Clippy, formatting, 16 architecture checks, and signed staging. Version-sensitive
  protocol golden tests now use 2.18. Reopened one preview process and read back
  the same four real work records. Native accessibility exposes the real Markdown
  history; the desktop screenshot bridge returned a white capture, so visual
  acceptance uses the native GPUI capture above rather than that bridge output.
- The prior Chief connection-recovery event remains. No new provider turn was sent;
  live usage ingestion is covered by the source-bound integration test, not claimed
  as a successful live model run. Historical metrics remain visibly unrecorded.

## Composer disclosure correction (2026-09-17)

Retain the last composer menu content while its disclosure height animates to zero.
Previously, closing the menu replaced its contents with an empty element before the
transition completed. This made the menu disappear abruptly despite the animated
container. The selected menu remains separate from the retained presentation.
GPUI suite: 165 passed, 5 ignored. Native end-to-end motion acceptance remains open.
