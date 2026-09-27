# Decodex upstream adoption: feature decisions

This is the product decision view for the fixed cutoff
`595cc91e8cbb1c2ca822d0311dcf12709410c582`. The scan covered 1,569 upstream commits;
the preservation audit covers 360 paths. Neither number counts adopted features.
Rows below group related behavior so the user can decide what Decodex needs.
They include restored baseline behavior as well as scan additions. The detailed
[adoption register](upstream-adoption-review.md) retains PR and source ownership.

Status: source reconciliation is complete; the reconciliation and inventory batches
through PR1666 are merged. Native limitations and signed desktop interaction remain
open. The [current signed artifact](signed-desktop-a15fe830-acceptance.md) has valid
contracts and signatures. Main-window access and normal exits are now verified;
ordinary composer interaction remains unverified. Implemented
does not mean fully accepted, installed or released. No removal is authorized here.

## Core compatibility to retain

| ID | Capability | Why Decodex needs it |
| --- | --- | --- |
| C01 | Native protocol, schema and installed CLI admission | Start the supported app-server and decode its actual responses without replacing native execution. |
| C02 | Exact conversation, turn, account and process ownership | Prevent stale history, catalog, approvals or replies from crossing tasks or accounts. |
| C03 | Complete input, media references and output | Preserve canonical attachments, long tool output, unfinished answers, public reasoning boundaries and copyable source. Optional rendering can change without losing data. |
| C04 | Drafts and uncertain-outcome recovery | Save original input before dispatch, retain receipts across restart, distinguish unsent from unknown and avoid duplicate submission. |
| C05 | Existing model/default/routing correctness | Preserve native inheritance, explicit user choices, directory identity, account routing and partial settings semantics. Optional selectors and automatic fallback are separate. |
| C06 | Approval and request correctness | Preserve complete evidence, native enforcement, explicit decisions and current pending-event identity. Removing optional review dashboards must not remove approval handling. |
| C07 | Existing native lifecycle observation | Preserve native turns, questions, goals, subagents, warnings, compaction and cancellation without inventing local user input or replaying work. |
| C08 | Storage and existing consumer maintenance | Preserve migration history, readable old receipts, unknown statistics, dependency compatibility and bounded caches. Retain effective voice/capture correctness while voice is present. |

Reduced-motion and VoiceOver behavior are correctness requirements for retained
animations. Removing an animation is optional; making retained animation ignore
accessibility preferences is not the equivalent subtraction. Likewise, source
access and readable text are core even if convenience copy buttons are removed.

## Implemented optional behavior for the user's decision

All rows have implementation or restored owners. Their linked records define
testing and acceptance limits. Decisions are **undecided**, not approved removal.

| ID | Optional behavior | When it is useful | Removal boundary |
| --- | --- | --- | --- |
| O01 | Manual task recap | Recover context after returning to a long task. | Remove recap generation and UI together; retain ordinary output I/O and readable saved history. [Recap](task-recaps.md) |
| O02 | Automatic recap | Generate eligible recaps after returning to a task; off by default. | Remove eligibility and preference independently of manual recap. Do not confuse this with maintenance scheduling. [Recap](task-recaps.md) |
| O03 | Edit an earlier prompt | Review canonical input, revert native history and explicitly resend edited input. | Resolve outstanding edit/send receipts first; retain native revert invalidation and request-size checks. [Prompt editing](prompt-editing.md) |
| O04 | MCP App widgets | View native tool resources and confirm widget tool calls. | Remove WebKit host and widget-specific service paths together; preserve ordinary MCP tools, approvals and uncertain call receipts. No event subscriptions or missing-URI discovery. [App UI](mcp-app-ui.md) |
| O05 | Native task resources and references | Add/list/remove native attachments and reference related tasks. | Keep attachments already present in canonical input and readable historical evidence. [Resources](upstream-media-acceptance.md) |
| O06 | Native archive restoration and installation suggestions | Restore an archived task or explicitly respond to a native plugin suggestion. | Remove these controls without deleting native history or making plugin installation automatic. [Adoption owners](upstream-adoption-review.md) |
| O07 | Saved task model selection | Change future turns of one task without changing global defaults. | Retain model inheritance, settings observation and legacy operation receipts. [Task model](task-model-panel-reconciliation.md) |
| O08 | Current-turn model/reasoning selection | Change later steps in an active turn when native support is enabled. | Remove its exact-turn publisher and UI together; keep saved-task settings and reviewer ownership. [Live model](live-model-control.md) |
| O09 | Exact model ID, effort and tier controls | Make explicit next-message choices, including an unlisted model. | Keep canonical execution intent and native validation; missing UI does not authorize changing saved task settings. [Exact model](exact-model-input.md) |
| O10 | Automatic ordinary model fallback | Select an advertised alternative for an idle task after a current recovery banner. | Remove automatic selection policy; retain manual selection, existing receipts and overload retry correctness. No input replay. [Fallback](ordinary-model-fallback.md) |
| O11 | Task permission and reviewer settings panels | Review native profiles and change task or active review settings. | Keep native policy enforcement, approval responses and unresolved operations readable. [Settings owners](settings-surface-reconciliation.md) |
| O12 | Per-task plugin selection | Enable or exclude native plugins for a particular task. | Remove the selector; retain native filtering, exact plugin identities and shared installation behavior. [Controls](optional-controls-reconciliation.md) |
| O13 | Hook, App connection and connector exposure editors | Configure native integrations and choose tool visibility. | Remove selected editor/command consumers together; retain shared journals for remaining writers and native discovery/refresh. [Settings](app-settings-owner-reconciliation.md) |
| O14 | Native goal/accounting display | Inspect a goal already owned by Codex. | Remove its panel; retain observation and recovery of native work already in progress. [Workspace](chief-workspace-reconciliation.md) |
| O15 | Guardian/misalignment review detail | Inspect native review evidence and explicit continuation state. | Remove optional details only; retain blocked state, native enforcement and required continuation acknowledgement. [Approval boundaries](large-approval-reconciliation.md) |
| O16 | Nonblocking provider-question timeout | Automatically send one empty response after grace/countdown unless the user interacts. | Remove timer policy without removing questions, explicit answers or asynchronous-card Skip. It is currently present. [Request owner](desktop-catalog-request-reconciliation.md) |
| O17 | Question and account-recovery notifications/actions | Bring attention to questions or expose explicit account recovery requests. | Preserve visible errors and recovery state; do not send notifications or external requests merely because a panel is removed. [Recovery](account-recovery-notices.md) |
| O18 | Public reasoning summaries and proposed-plan display | Show native public progress alongside the answer. | Hide optional display without exposing raw reasoning or discarding unfinished answer evidence. [Rendering](native-message-rendering-reconciliation.md) |
| O19 | Math, Mermaid, weather and rich preview presentation | Read structured results directly in the conversation. | Keep source text, explicit media access and privacy filtering. Weather is presentation, not a new execution owner. [Rendering](rich-markdown-rendering.md) |
| O20 | Native child inspection/input presentation | Inspect native descendants through existing native capabilities. | Keep child ownership, approvals and parent attribution. Do not replace native agent execution. Human MCP input has a known native limit. [Child qualification](native-child-mcp-qualification.md) |
| O21 | Ordinary direct-conversation History workbench | Use a separate ordinary conversation/settings view. | Currently not exposed by normal startup navigation. Remove its complete consumer only after mapping shared routing, drafts and recovery dependencies. Do not add navigation just for acceptance. [Scope](initial-model-source-recovery.md) |
| O22 | Voice preference picker | Select native preferences for future calls. | Remove this control separately from effective settings and capture correctness for retained voice. [Voice settings](voice-settings.md) |
| O23 | Catalog access-program notices | Inspect native model metadata. | Remove the notice and dedicated projection; preserve model discovery. This display grants no entitlement and is not a Daybreak selector. [Controls](optional-controls-reconciliation.md) |
| O24 | Native provider sign-in recovery history | Inspect AWS/Bedrock recovery notices if those providers are used. | Baseline restoration, not a new scan feature. Remove this specialized display without removing general authentication diagnostics. [Provider scope](provider-auth-recovery-history.md) |
| O25 | Existing Live voice and dictation | Use subscription audio in the composer. | Baseline product scope, not entirely new scan adoption. Any removal must retire capture safely and keep historical transcripts and drafts readable. Physical audio acceptance remains open. [Voice](voice-input.md) |
| O26 | Existing automatic quota activation | Apply the existing account activation policy. | Separate product scope; retained activation must preserve account policy, residency and routing. Do not remove routing correctness from other consumers. [Routing](native-policy-routing-reconciliation.md) |

For a first subtraction review, O21 and O24 have explicit applicability questions;
O04, O02 and O25 carry additional acceptance or runtime obligations. O23's access
notice is informational. These are review priorities, not claims that the user
does not need them. The user decides which rows remain.

## Proposals and native capabilities not delivered as local product features

| Item | Current disposition |
| --- | --- |
| Full Analytics, Top chats, expanded Summary/activity and Plan history | Optional research; local reports/dashboard are unimplemented. Existing task estimates and profile statistics are different consumers. [Usage scope](usage-scope-reconciliation.md) |
| Collaboration-mode catalog/selector | Schema support and native mode restoration do not establish a local selector. No production catalog consumer was found in this review. |
| Daybreak preference/program selector | Native response/start qualification exists; no Decodex preference control or entitlement grant is implemented. |
| Safety-buffering retry/fork UI | Not implemented; separate from overload retry and ordinary model fallback. |
| Generic experimental-feature editor and memory readiness/version controls | Discovery or a read-only memory flag is not a complete editor or readiness workflow. |
| Native blank-session/worktree creation and TUI task grouping/history search | Local draft support does not establish these flows. Keep optional applicability separate from existing draft acceptance. [Overview](command-center-upstream.md) |
| Automatic native user verification | Current Decodex host identity has no qualified activation/registration route; do not advertise support from platform or schema presence. |
| Remote image upload/resolution, MCP App streams and widgets without captured URIs | Current consumers retain opaque references and explicit unavailable states; these additional consumers are unsupported/unimplemented. |
| TUI daemon startup, starfield/keymaps, Windows provisioning, Linux mounts, native Guardian/Code Mode internals | Native or platform-specific owners without an equivalent local consumer. No duplicate runtime, security evaluator or platform port is added for parity. |

## Known limitations and delivery gates

1. Explicit Flex changed through native settings is not qualified across cold
   resume; configured Flex passes. Do not replay settings to hide that difference.
2. Native child MCP browser-auth/user-input markers return empty acceptance instead
   of required root handoff on the tested binary. Keep the strict failing fixture.
3. Voice WebRTC/media, physical audio and late remote caption identity remain
   qualified only to the precise limits of their records.
4. Independent Decodex browser/device-code enrollment has no native enrollment-policy
   authority integration. Native execution admission separately enforces its own
   authentication restrictions, including external ChatGPT credentials. A global
   enrollment-policy owner remains an unimplemented scope decision; local UI
   enforcement is not claimed.
5. Main-window interaction and history-edit draft restart are verified. Ordinary
   composer interaction remains unverified and is deferred until last by user
   instruction. A later fixture exposed the separate composer accessibility tree;
   that observation does not establish successful input. Broader lifecycle and
   physical audio acceptance remain open.

See [native limits](source-preservation-native-limits.md) and
[signed acceptance](signed-desktop-a15fe830-acceptance.md). The current known native
binary hash is recorded there. Source classifications and ordinary passing tests
must not erase these limitations.

## Future maintenance rule

Necessary compatibility fixes must name the existing Decodex consumer and explain
the failure they repair. New controls, workflows, dashboards or policies are
optional proposals: notify the user with value, dependency and maintenance impact
before implementing them. A broad upstream scan is discovery, not authorization
to implement every native capability. Maintenance remains paused, including after
this manual pass. No deletion, feature enablement or scheduled resumption occurs
as a result of this decision inventory.
