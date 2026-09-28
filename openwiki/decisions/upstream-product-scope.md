---
type: Reference
tags: [decodex, architecture]
title: "Retained upstream product capabilities"
description: "The retained core and optional capabilities, O24 retirement, and future upstream adoption decisions."
verified:
  - by: openwiki/0.6.0
    at: 2026-09-28T06:22:45.949Z
sources:
  - id: openwiki-source-e32adebfd6d3bf27dc186bad
    resource: repo://crates/decodex-runtime/src/agent/tests/auth_recovery.rs
  - id: openwiki-source-8d4b61fd83ed007c18390abe
    resource: repo://docs/archive/upstream-2026-09/upstream-feature-decisions.md
generated: { by: "codex", at: "2026-09-28T06:22:45.949Z" }
---


# Retained upstream product capabilities

Decision: the user retained **O01–O23 and O25–O26** and retired **O24** on 2026-09-27. This is the current product boundary for future maintenance. Retention does not claim complete runtime qualification, installation or release. The fixed scan and restored baseline are described in the [archive](../../docs/archive/upstream-2026-09/README.md).

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

## Optional behavior and recorded user decisions

The user retained every optional row except O24. Their linked records define
testing and acceptance limits. O24 is retired; only saved-history reading remains.

| ID | Optional behavior | When it is useful | Removal boundary |
| --- | --- | --- | --- |
| O01 | Manual task recap | Recover context after returning to a long task. | Remove recap generation and UI together; retain ordinary output I/O and readable saved history. [Recap](../../docs/archive/upstream-2026-09/task-recaps.md) |
| O02 | Automatic recap | Generate eligible recaps after returning to a task; off by default. | Remove eligibility and preference independently of manual recap. Do not confuse this with maintenance scheduling. [Recap](../../docs/archive/upstream-2026-09/task-recaps.md) |
| O03 | Edit an earlier prompt | Review canonical input, revert native history and explicitly resend edited input. | Resolve outstanding edit/send receipts first; retain native revert invalidation and request-size checks. [Prompt editing](../../docs/archive/upstream-2026-09/prompt-editing.md) |
| O04 | MCP App widgets | View native tool resources and confirm widget tool calls. | Remove WebKit host and widget-specific service paths together; preserve ordinary MCP tools, approvals and uncertain call receipts. No event subscriptions or missing-URI discovery. [App UI](../../docs/archive/upstream-2026-09/mcp-app-ui.md) |
| O05 | Native task resources and references | Add/list/remove native attachments and reference related tasks. | Keep attachments already present in canonical input and readable historical evidence. [Resources](../../docs/archive/upstream-2026-09/upstream-media-acceptance.md) |
| O06 | Native archive restoration and installation suggestions | Restore an archived task or explicitly respond to a native plugin suggestion. | Remove these controls without deleting native history or making plugin installation automatic. [Adoption owners](../../docs/archive/upstream-2026-09/upstream-adoption-review.md) |
| O07 | Saved task model selection | Change future turns of one task without changing global defaults. | Retain model inheritance, settings observation and legacy operation receipts. [Task model](../../docs/archive/upstream-2026-09/task-model-panel-reconciliation.md) |
| O08 | Current-turn model/reasoning selection | Change later steps in an active turn when native support is enabled. | Remove its exact-turn publisher and UI together; keep saved-task settings and reviewer ownership. [Live model](../../docs/archive/upstream-2026-09/live-model-control.md) |
| O09 | Exact model ID, effort and tier controls | Make explicit next-message choices, including an unlisted model. | Keep canonical execution intent and native validation; missing UI does not authorize changing saved task settings. [Exact model](../../docs/archive/upstream-2026-09/exact-model-input.md) |
| O10 | Automatic ordinary model fallback | Select an advertised alternative for an idle task after a current recovery banner. | Remove automatic selection policy; retain manual selection, existing receipts and overload retry correctness. No input replay. [Fallback](../../docs/archive/upstream-2026-09/ordinary-model-fallback.md) |
| O11 | Task permission and reviewer settings panels | Review native profiles and change task or active review settings. | Keep native policy enforcement, approval responses and unresolved operations readable. [Settings owners](../../docs/archive/upstream-2026-09/settings-surface-reconciliation.md) |
| O12 | Per-task plugin selection | Enable or exclude native plugins for a particular task. | Remove the selector; retain native filtering, exact plugin identities and shared installation behavior. [Controls](../../docs/archive/upstream-2026-09/optional-controls-reconciliation.md) |
| O13 | Hook, App connection and connector exposure editors | Configure native integrations and choose tool visibility. | Remove selected editor/command consumers together; retain shared journals for remaining writers and native discovery/refresh. [Settings](../../docs/archive/upstream-2026-09/app-settings-owner-reconciliation.md) |
| O14 | Native goal/accounting display | Inspect a goal already owned by Codex. | Remove its panel; retain observation and recovery of native work already in progress. [Workspace](../../docs/archive/upstream-2026-09/chief-workspace-reconciliation.md) |
| O15 | Guardian/misalignment review detail | Inspect native review evidence and explicit continuation state. | Remove optional details only; retain blocked state, native enforcement and required continuation acknowledgement. [Approval boundaries](../../docs/archive/upstream-2026-09/large-approval-reconciliation.md) |
| O16 | Nonblocking provider-question timeout | Automatically send one empty response after grace/countdown unless the user interacts. | Remove timer policy without removing questions, explicit answers or asynchronous-card Skip. It is currently present. [Request owner](../../docs/archive/upstream-2026-09/desktop-catalog-request-reconciliation.md) |
| O17 | Question and account-recovery notifications/actions | Bring attention to questions or expose explicit account recovery requests. | Preserve visible errors and recovery state; do not send notifications or external requests merely because a panel is removed. [Recovery](../../docs/archive/upstream-2026-09/account-recovery-notices.md) |
| O18 | Public reasoning summaries and proposed-plan display | Show native public progress alongside the answer. | Hide optional display without exposing raw reasoning or discarding unfinished answer evidence. [Rendering](../../docs/archive/upstream-2026-09/native-message-rendering-reconciliation.md) |
| O19 | Math, Mermaid, weather and rich preview presentation | Read structured results directly in the conversation. | Keep source text, explicit media access and privacy filtering. Weather is presentation, not a new execution owner. [Rendering](../../docs/archive/upstream-2026-09/rich-markdown-rendering.md) |
| O20 | Native child inspection/input presentation | Inspect native descendants through existing native capabilities. | Keep child ownership, approvals and parent attribution. Do not replace native agent execution. Human MCP input has a known native limit. [Child qualification](../../docs/archive/upstream-2026-09/native-child-mcp-qualification.md) |
| O21 | Ordinary direct-conversation History workbench | Use a separate ordinary conversation/settings view. | Currently not exposed by normal startup navigation. Remove its complete consumer only after mapping shared routing, drafts and recovery dependencies. Do not add navigation just for acceptance. [Scope](../../docs/archive/upstream-2026-09/initial-model-source-recovery.md) |
| O22 | Voice preference picker | Select native preferences for future calls. | Remove this control separately from effective settings and capture correctness for retained voice. [Voice settings](../../docs/archive/upstream-2026-09/voice-settings.md) |
| O23 | Catalog access-program notices | Inspect native model metadata. | Remove the notice and dedicated projection; preserve model discovery. This display grants no entitlement and is not a Daybreak selector. [Controls](../../docs/archive/upstream-2026-09/optional-controls-reconciliation.md) |
| O24 | Retired: native provider sign-in recovery history | Not needed by the user. | New notification consumption and receipt writes are removed. Keep saved receipts readable and preserve native authentication. [Retirement](../../docs/archive/upstream-2026-09/provider-auth-recovery-history.md) |
| O25 | Existing Live voice and dictation | Use subscription audio in the composer. | Baseline product scope, not entirely new scan adoption. Any removal must retire capture safely and keep historical transcripts and drafts readable. Physical audio acceptance remains open. [Voice](../../docs/archive/upstream-2026-09/voice-input.md) |
| O26 | Existing automatic quota activation | Apply the existing account activation policy. | Separate product scope; retained activation must preserve account policy, residency and routing. Do not remove routing correctness from other consumers. [Routing](../../docs/archive/upstream-2026-09/native-policy-routing-reconciliation.md) |

The user completed this subtraction review: retire O24 and retain the other
optional capabilities. Do not revive earlier removal suggestions as pending work.

## Proposals and native capabilities not delivered as local product features

| Item | Current disposition |
| --- | --- |
| Full Analytics, Top chats, expanded Summary/activity and Plan history | Optional research; local reports/dashboard are unimplemented. Existing task estimates and profile statistics are different consumers. [Usage scope](../../docs/archive/upstream-2026-09/usage-scope-reconciliation.md) |
| Collaboration-mode catalog/selector | Schema support and native mode restoration do not establish a local selector. No production catalog consumer was found in this review. |
| Daybreak preference/program selector | Native response/start qualification exists; no Decodex preference control or entitlement grant is implemented. |
| Safety-buffering retry/fork UI | Not implemented; separate from overload retry and ordinary model fallback. |
| Generic experimental-feature editor and memory readiness/version controls | Discovery or a read-only memory flag is not a complete editor or readiness workflow. |
| Native blank-session/worktree creation and TUI task grouping/history search | Local draft support does not establish these flows. Keep optional applicability separate from existing draft acceptance. [Overview](../../docs/archive/upstream-2026-09/command-center-upstream.md) |
| Automatic native user verification | Current Decodex host identity has no qualified activation/registration route; do not advertise support from platform or schema presence. |
| Remote image upload/resolution, MCP App streams and widgets without captured URIs | Current consumers retain opaque references and explicit unavailable states; these additional consumers are unsupported/unimplemented. |
| TUI daemon startup, starfield/keymaps, Windows provisioning, Linux mounts, native Guardian/Code Mode internals | Native or platform-specific owners without an equivalent local consumer. No duplicate runtime, security evaluator or platform port is added for parity. |


## Maintenance implications

Repair existing retained consumers when an upstream change breaks compatibility or correctness. Propose new optional controls or policies before implementing them. Do not restore O24's notification consumer or receipt writer; keep saved historical receipts readable and native authentication intact. Do not revive earlier suggestions to remove other rows as pending user decisions.

The user authorized daily upstream maintenance on 2026-09-28. This authorization covers existing-capability maintenance, not new optional features or app installation/release. See [maintenance](../operations/codex-upstream-autopilot.md) and [acceptance limits](../testing/upstream-acceptance-boundaries.md).
