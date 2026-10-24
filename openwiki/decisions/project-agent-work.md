---
type: Decision
title: "Project, Agent and Work contract"
description: "Accepted architecture, shared English vocabulary, minimum Work and Collaboration contracts, recovery, and global Guardian controls."
tags: ["decodex", "architecture", "agents", "contracts", "guardian"]
verified:
  - by: openwiki/0.7.2
    at: 2026-10-22T20:51:42.199Z
sources:
  - id: openwiki-source-a053194f0c54bd9453573384
    resource: repo://crates/decodex-codex/src/app_server_client/permissions.rs
  - id: openwiki-source-098c6a9588eb0a1e1188f475
    resource: repo://crates/decodex-runtime/src/agent_live_settings.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-cd44bbfa50ac15a596e824a2
    resource: repo://crates/decodex-runtime/src/agent/guardian.rs
  - id: openwiki-source-2fd274a65db230908814f7da
    resource: repo://crates/decodex-runtime/src/agent/native_turns.rs
  - id: openwiki-source-dc98e0a4203f94a608c96afd
    resource: repo://database/migrations/0054_agent_delegations.sql
  - id: openwiki-source-8fb98024229a4675beaa73d0
    resource: repo://database/src/agent_deliveries.rs
  - id: openwiki-source-3030ced79067a3721a08b107
    resource: repo://database/src/agent_native_turns.rs
  - id: openwiki-source-9782a193c0b21f967135010a
    resource: repo://database/src/agent/delegation.rs
generated: { by: "codex", at: "2026-10-22T20:51:42.199Z" }
---

# Project, Agent and Work contract

## Decision and implementation status

Status: **Accepted design; implementation pending.** The user approved this architecture, global Guardian controls, and the English vocabulary below. This page is the canonical decision for that scope. It supersedes earlier design proposals that require fixed L0/L1/L2 management levels, mandatory routing through Main, permanent component managers, or several concurrent conversations that share one Agent identity.

This decision applies to general-purpose work, including research, planning, writing, operations, analysis and software. It does not authorize installation, release, changes to a maintenance schedule, or automatic execution of all historical optional proposals. It does not certify an installed application. A documentation update does not migrate stored conversations, apply native settings, or enable Review Details.

Normative sections define the target. [Current implementation and migration](#current-implementation-and-migration) identifies reusable behavior and required changes. Older qualification records retain their original source, artifact and behavior limits. An older record that calls Review Details unselected describes its qualification scope; the product decision on this page is now accepted.

## Canonical vocabulary

Use one English vocabulary in the UI, code, database and documentation. Identifier formatting can differ, for example ProjectLead and Project Lead. Do not introduce a second set of business aliases.

| Term | Definition |
| --- | --- |
| Project | A continuing context for related Work, Materials and decisions. It is not an executing Agent and does not require a Git repository. |
| Agent | A persistent collaborator created by the user, with one current native conversation and an explicit mandate. |
| Assistant | The personal-assistance responsibility of an Agent. It replaces Main as a target product term. |
| Project Lead | The responsibility of an existing Agent selected to coordinate a Project. It is not a separate Agent class or management level. |
| Work | A durable outcome with one accountable Agent, current requirements and completion criteria. It is not a native thread or turn. |
| Subagent | A native Codex helper dispatched for bounded execution. Preserve the native term. |
| Collaboration | Cooperation between independent Agents without changing their ownership relationship. |
| Collaboration Request | A scoped request for a contribution, with acceptance, Results and an explicit assessment. It is a record associated with Work, not another management layer. |
| Material | An authorized input, such as a file, link, reference or decision. |
| Result | An output or outcome associated with specific Work requirements. A Result can await assessment. |
| Guardian | The native action-review mechanism, controlled from global Decodex settings in the target. |
| Review | Guardian's assessment of one action. It is separate from assessment of a Work Result. |
| Review Details | Supported detailed native reviewer records associated with a Review. |

Native Thread, Turn and Goal keep their distinct execution meanings and protocol names. Existing Main, manager, worker, Task and Goal storage names are compatibility facts until their migrations complete. They do not define the target business model. In the target, Project replaces Workspace as the business term; existing directory and membership data must survive migration.

## Responsibilities and native ownership

The core business objects are Project, Agent and Work. Materials, Results, decisions, delegation records and Collaboration Requests belong to those objects. Do not create a parallel role system or execution engine for them.

| Owner | Responsibility |
| --- | --- |
| User | Create persistent Agents, select responsibilities, set goals and authority, and decide changes that exceed delegation. |
| Assistant | Help the user directly and summarize or coordinate authorized Work when asked. It is an optional entry point, not a mandatory router or reviewer. |
| Project Lead | Discuss requirements with the user, organize Work and maintain relevant Project decisions. Store each Work owner explicitly. |
| Work owner | Arrange execution, inspect Results, request repair, manage blockers and make the completion judgment within the user's authority. |
| Independent contributing Agent | Accept or decline a Collaboration Request and own the agreed contribution. It retains its own conversation and execution permissions. |
| Native Codex | Own native conversations, Subagent execution, messages, context management, native Goals, execution permission and enforcement. |
| Decodex service | Own durable business relationships, scoped access, input provenance, delivery receipts, state validation, recovery and presentation. |

An Agent can participate in several Works. Selecting an existing Agent as Project Lead does not create another conversation or manager. The user can talk directly to that Agent through discussion, execution, verification and changes. Do not create permanent research or review Agents merely because a Work needs those activities. Use Subagents when a bounded contribution benefits from separate execution; simple Work can remain with its owner.

Project membership does not grant unrestricted conversation access, broadcast every decision, or send every Result to the Assistant. A Work context selects relevant requirements and authorized Materials. It does not erase the Agent's existing conversation or establish a separate sandbox. Use a separate native execution context when hard isolation is required.

## Contract 1: Bind Work, requirements, execution and Results

Each Work retains:

- Stable identity, Project association and one accountable Agent.
- Goal, current requirements, completion criteria and the source of authoritative decisions.
- Business state, blocker or hold reason, and the event or dependency needed to continue.
- Relevant Materials, Results, assessments and their limits.

Every contribution retains its Work reference, the instruction actually issued, the requirements it used, its target and its return route. A Collaboration Request also has its own stable identity. Instructions can use existing immutable event references; a separate global version service is not required.

The required chain is **Work -> requirements -> instruction -> execution -> Result -> assessment**. Bind native evidence to its exact thread, turn and item where applicable. Bind mutable artifacts to a specific version or digest. Never use an Agent's latest reply as the Result of every Work that Agent owns.

Native Subagents remain native. Associate their actual spawn and message events with the Work; do not promote them into persistent local Agents or launch them through a second local executor. If native admission occurs before a local binding is saved, recover the binding from exact native evidence. An ambiguous binding stays Unknown and does not authorize another spawn.

Authority remains attached to the source. A contributor's suggestion is not a new user instruction. A Collaboration Request cannot grant the recipient unrestricted history access, change its execution permissions, or be replayed as user input. Reuse native Material storage and scoped references instead of copying whole conversations into a shared brain.

## Contract 2: Separate delivery, acceptance, return and assessment

The minimum Work states are Open, Blocked, Completed and Cancelled. A pending Result can produce an Awaiting Assessment activity label while the Work remains Open. A native turn ending does not itself change Work to Completed.

| Transition | Authority and condition |
| --- | --- |
| Open -> Blocked | The owner records a concrete missing decision, permission, dependency, unavailable source or hold. |
| Blocked -> Open | The recorded condition is satisfied and continuation remains authorized. A user hold requires explicit release. |
| Open -> Completed | The current owner supplies an assessment against current requirements and exact Result evidence. |
| Open or Blocked -> Cancelled | An authorized cancellation records the affected scope and stops new related admission. |
| Completed -> Open | A new authorized requirement or explicit reassignment reopens the Work. Late old receipts cannot do this. |

A Collaboration Request progresses through Pending, Accepted, Returned and Closed. A Closed request retains its outcome, such as contribution used, not used, declined, cancelled or failed. Repair appends a new instruction to the same request and starts another assessment cycle. Preserve earlier instructions and Results.

Delivery has separate facts: not sent, admitted or delivered, explicitly rejected, or Unknown. These are not business acceptance. Native Running, Waiting, Failed and Interrupted observations are execution facts, not a second Work scheduler.

| Action | Responsible party |
| --- | --- |
| Issue a request or repair instruction | Work owner. |
| Accept, decline, or explain inability to contribute | Receiving independent Agent. A quick response can accept and return a Result in one operation. |
| Record native execution admission or failure | Service, from exact native evidence. Native admission does not prove Result quality. |
| Return a contribution | Bound executor or receiving Agent, with instruction and Result references. |
| Use, reject, or request repair of a contribution | Requesting Work owner. |
| Judge the complete Work | Its current accountable Agent. |

Use two execution paths with this shared attribution contract:

- Native Subagent contributions use native spawn, message, follow-up, wait, interruption and recovery behavior. Do not add a second mailbox for those native messages.
- Independent Agent contributions use the existing Decodex inbox and root-conversation admission path, extended with request identities and explicit acceptance. Preserve tool authority for application-origin input.

The selected native interface must actually support each operation. A helper follow-up API must not be assumed to be a generic bus for unrelated root conversations. The [inspected native implementation](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/core/src/agent/control/api.rs#L126-L147) rejects a follow-up that targets a root. This source reference does not select or qualify that runtime for Decodex.

When an independent Agent is busy, a durable request can wait without being reported as accepted. The recipient can decline or identify a conflict. The requester then chooses another path or records a blocker. Do not add an automatic global priority optimizer or require the Assistant to arbitrate every request. Preserve user-input priority, explicit pauses and current native ownership gates.

## Contract 3: Apply changes to the affected scope

Record a meaningful requirement change with its authority, affected Works and current requirement reference. Ordinary discussion need not create a Work or revision. An owner can refine execution within its mandate; changing the user's budget, permissions or objective requires the relevant authority.

For each affected contribution, preserve the old instruction and append the change. Record separately that the change was saved, delivered, and acknowledged or acted on. Check the current requirement reference before new local admission and before accepting completion.

Use native follow-up or interruption where supported. An in-flight tool may already have acted; a message does not guarantee immediate cancellation or rollback. Record unresolved execution instead of reporting that every helper has switched to the new requirements.

An old Result can be retained and reused only after the owner checks which parts remain valid. That check links the original evidence to the current requirements; it does not rewrite the historical Result as if it was produced under the new instruction. Old completion cannot close a newer request.

Cancel only the related contribution. Cancelling one Collaboration Request must not interrupt the independent Agent's unrelated Work. A late Result remains history and cannot reopen cancelled Work. Shared Project decisions must identify affected Work explicitly; unrelated Projects do not inherit a requirement change.

## Contract 4: Complete Work through a current assessment

The owner's assessment states what was delivered, which requirements it meets, the supporting evidence and material limits. The service validates current ownership, the expected requirement reference and related Result identities in the same transaction as the completion write. Reject a stale or foreign assessment without discarding its evidence.

Required contributions must be assessed. Unneeded contributions can be explicitly cancelled or marked not used. Work completion need not wait for every redundant helper to succeed. Self-produced Results use the same evidence and assessment rule as delegated Results.

Subagent completion, contributor return, Guardian approval, user read status and Work completion remain distinct facts. Human acceptance is required only when the agreed outcome or a missing decision requires it. The Assistant is not a mandatory reviewer. Use bounded additional review when its value justifies it; do not create a permanent review role by default.

Completion criteria are domain-specific. A researched venue candidate does not prove a reservation. A draft itinerary does not prove a trip occurred. If the user asked for the reservation, the Work remains open until that outcome is evidenced or explicitly changed.

## Failure and restart recovery

Persist intent and identity before a local external submission. A transport timeout does not prove that execution did not start. Local deduplication does not guarantee exactly-once effects in an external service.

| Condition | Recovery contract |
| --- | --- |
| Known unsent or Busy | Keep the pending instruction. Submit only when the exact target and admission rules permit it; do not report acceptance. |
| Submission without an acknowledgment | Mark Unknown. Reconcile exact receipts and native history before any replay. Missing evidence is not proof of non-execution. |
| Native execution still active | Reattach to its original identity and observe it. Do not start another executor. |
| Native execution ended but its notification was missed | Recover the exact terminal Result once and leave assessment pending. |
| Result saved but owner assessment missing | Restore the pending assessment under current requirements and authority. |
| Duplicate or late event | Deduplicate by stable source identity. Preserve historical evidence without repeating acceptance or closing newer instructions. |
| Executor cannot be recovered | Record the unavailable source and affected contribution. Establish the old execution state before arranging replacement execution. |
| Recipient declines, is archived, or is owned elsewhere | Retain the request and reason. Do not auto-unarchive, create a replacement identity, or bypass the ownership gate. Continue independent Work. |
| User paused or cancelled | Preserve that decision. Restart is not new authority to continue. |
| Material is unavailable | Block only the current action that needs it. Preserve earlier assessed outcomes with an explicit evidence-availability limit. |
| External effect is uncertain | Reconcile with the external owner. Do not repeat a booking, message or payment merely because the local process restarted. |

On service restart, restore process ownership and native bindings first. Read the needed native state, exact turn or item history and effective permissions. Reconcile local receipts, then handle known pending input and Results. Reuse existing admission fences, inbox wakes and native continuation; do not create a polling controller or synthetic user turn for every open Work. A stopped or unsupported native continuation remains visible as a concrete blocker.

Native context compaction remains native-owned. Before a new assignment, recovery, assessment or other material decision, load the current Work requirements and relevant Project decisions. Do not depend on a compacted summary alone or broadcast entire transcripts to every Agent. This mechanism improves attribution; it does not prove the semantic correctness of a model's analysis.

## Global Guardian decision

Guardian controls are global for Decodex, with no per-Agent override. Apply the same desired Review preference to the supported native sources used by all Agents and Projects. Managed native requirements still apply. A source that cannot adopt the global preference reports its actual effective value and reason; it does not gain an editable local exception.

Keep the following separate:

| Concern | Target behavior |
| --- | --- |
| Review preference | Global Ask Me or Automatic Review selection, using supported native settings. |
| Execution permissions | Native filesystem, network, tool and approval rules. A Guardian switch does not grant Full Access. |
| Review observation | Existing action, decision, risk and rationale remain readable for the specific operation. |
| Review Details | Accepted global opt-in for supported private reviewer output and interruption records. It is diagnostic observation, not another reviewer. |

An effective setting requires native readback. Store the desired global setting and the per-source application receipt separately from observed effective facts. These observations are not per-Agent controls. A save acknowledgment does not establish application to every loaded conversation. Display pending, overridden, unsupported, rejected or Unknown outcomes truthfully. Reconnect and settings recovery must not silently reapply creation defaults or replay uncertain writes.

Changing the Review preference does not answer an already-pending approval. Ask Me requests remain human decisions; another Agent cannot impersonate that approval. Ordinary factual questions can be investigated or escalated within authority, but they do not confer execution permission. Automatic Review routes eligible approvals to the native reviewer and can deny them. Full Access remains a separate permission choice under native policy; approval_policy=never alone does not grant broader access.

Subagents follow the actual native parent permission rules. An independent Agent keeps its own native execution boundary, subject to the global Review preference where supported. A Work label does not create an isolated sandbox inside a shared conversation. The architecture does not promise simultaneous, independently enforced permission modes for several Works in one native execution context.

### Review Details lifecycle

The user selected this capability as part of the global design. Keep its opt-in in global Guardian settings, not on individual Agents. Before implementation, qualify the native event subscription and configuration against the exact selected binary and helpers. Do not copy the reviewer or enforcement engine.

Observe only records exposed by the supported native protocol. Preserve the private reviewer's thread and turn identity, and associate records with the actual Review and target operation. Never attach them to the latest parent turn or latest Review by guesswork. A missing association remains explicit.

Private reviewer records do not enter the parent's canonical conversation, grant authority, complete Work, start another turn, or trigger action replay. A private reviewer's interruption must not complete or clear the parent's active turn. Use bounded subscription and buffering with the existing authorized storage and retention boundary. Reconnection must expose record gaps rather than invent a complete transcript. Turning observation off stops new optional observation; it does not decide pending approvals or require deletion of existing action-review history.

Review Details may contain structured output rather than a fuller explanation. The UI must not promise access to complete internal reasoning. Keep normal action conclusions concise and make detailed records available on demand. The [native event forwarding boundary](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/core/src/session/mod.rs#L2763-L2790) retains reviewer identity and bypasses parent history. This is inspected source evidence, not installed-app acceptance.

## Two Projects and four Works

This example illustrates the contract. The places, amounts and outcomes are assumptions, not real quotes or execution receipts. The agreed outcomes are executable planning Results; reservation and payment are outside this example.

| Project | Project Lead and Work owner | Work |
| --- | --- | --- |
| Community Market | Sage | A1: Venue, event plan and visitor routes. |
| Community Market | Sage | A2: Budget and preparation checklist. |
| Family Trip | Iris | B1: Two-day itinerary and transport. |
| Family Trip | Iris | B2: Accommodation comparison and cost plan. |

1. The user discusses the goals directly with Sage and Iris. Work becomes explicit when an outcome and criteria are clear. The Assistant need not participate.
2. Each owner chooses direct execution or authorized native Subagents. The authority to delegate is explicit and does not depend on Ultra. Actual delegation behavior still needs qualification with supported model and native settings.
3. The user has permitted a bounded exchange of public venue and transport Materials. Sage asks Iris for those Materials for A1. The request waits while Iris is busy and becomes Accepted only when she accepts it. Her family information is not shared. This contribution is a Collaboration Request, not another permanent Agent or a fifth independent Work.
4. The user changes the market from 60 people to 80 people, fully indoors, with the same budget. Sage updates A1 and A2 and their affected instructions. B1 and B2 keep their requirements. Old venue capacity evidence cannot close A1; unchanged transport evidence can be checked for reuse.
5. Sage finds that insurance in A2 still uses 60 people and returns the same contribution for repair. Iris finds that a proposed hotel does not fit B1's accessible route and arranges a compatible revision. The original Work identities and owners remain.
6. Each owner checks current Results against the agreed criteria and closes its Works. The user can follow each Result back to requirements, Materials, Collaboration Requests and assessment. The plans do not claim that a venue or hotel has been booked.

## Current implementation and migration

The following are current source facts, not proof that the complete target is implemented:

- AgentCoordinator::create_goal stores a separate outcome without creating a native execution conversation. This is a reusable Work foundation.
- Current conversation creation still requires a personal root and uses parent_goal_id. The target must remove the resulting mandatory business hierarchy while preserving identities and history.
- The agent_delegations table uses work_id as its primary key. Its current outstanding-assignment relation cannot represent several independent requests to one Agent without extension. Its older-Result protection remains useful.
- dispatch_input preserves application tool authority for delegation and wakes. It does not turn those inputs into user authorization.
- recover_native_turns and native terminal receipts recover exact existing execution without replaying local input. Native child ownership uses verified spawn ancestry; a fork alone does not grant it.
- Current deliveries aggregate conversation progress and latest reply data. A separate passive Work does not yet have the complete target requirement, native Result and assessment association.
- Current permission selection submits a named native profile separately from the reviewer. Live reviewer edits target the active turn. These controls are not a persistent global Guardian implementation.
- Current Guardian denial approval uses the exact saved Review and does not start a turn or retry the action. Guardian observations are evidence, not Work completion.

| Area | Decision |
| --- | --- |
| Agent identity, exact native bindings, canonical input and uncertainty fences | Retain. |
| Native Subagent execution, activity, usage, context and approval ancestry | Retain under native ownership; add Work attribution. |
| Separate outcome records, scoped references, dependencies, inbox and assessments | Retain as foundations; extend their business associations. |
| Independent Collaboration Requests | Add stable per-request identity, acceptance, targeted Results, repair and cancellation to existing transport. |
| Requirement changes and completion | Add current-requirement checks and atomic owner assessment; preserve older evidence. |
| Project integration | Preserve current directory and membership data. Adopt supported native Project identity and catalog ownership only after exact-interface qualification; retain Decodex Work and responsibility associations. |
| Global Guardian and Review Details | Implement the accepted global controls and native observation boundary. Preserve native enforcement and truthful per-source readback. |
| Default temporary execution through agent_create_work and agent_continue_worker | Retire from the new delegation path only after native replacement behavior is qualified and existing consumers and in-flight Work are handled. |
| Automatic creation of persistent subordinate managers | Remove from the target Agent tool surface. User-created persistent Agents remain supported. |
| Existing automation, external evidence, native recovery and independent-request wakes | Preserve their real consumers; do not delete them as if they were all duplicate Subagent scheduling. |

Do not replace an existing managed worker with a differently labelled native child. Preserve unfinished Work, historical identities and supported compatibility readers until their consumers are migrated. New business code uses the canonical names; protocol compatibility must remain explicit.

## Qualification and completion of implementation

Use the repository-selected native artifact, its helpers and generated schema. A source commit, version label, ancestry or matching schema alone does not prove behavior. Source review, executed tests, merged delivery and installed-app acceptance are separate evidence.

Implementation must cover these meaningful cases:

- One Agent handles two Works; a Result for one cannot complete the other. Self-produced and delegated Results follow the same rule.
- Busy independent requests survive restart without duplicate admission or a false Accepted status. Wrong recipients and ungranted private references are rejected.
- Lost acknowledgment is reconciled without replay. Duplicate and missed native terminal events recover one Result and one pending assessment.
- A change during execution rejects stale completion. Valid old Materials can be explicitly reused. Repair preserves identity and cancels only the affected contribution.
- Subagent completion can reach an idle owner through supported native behavior without a second local wake. Pauses, cancellation and active-turn ownership remain effective.
- Global Guardian preferences are checked against actual effective native settings before and after restart. Managed restrictions, pending human approvals, inherited child policy and uncertain setting writes retain their boundaries.
- Optional Review Details preserve private reviewer identity, parent-history isolation, interruption scope and bounded recovery. Observation never authorizes execution or retries an action.

Existing relevant tests include database agent/tests/delegation.rs, runtime agent/tests/completion_contract.rs, agent/tests/native_agent_wake.rs, account_launch/agent_process_native_permission_tests.rs, account_launch/agent_process_native_guardian_evidence_tests.rs and agent/tests/guardian.rs. Read test bodies and verify actual matched test counts when executing them. Their existence or historical receipts do not establish the new global behavior. This documentation task did not execute application or native runtime tests.

See [current Agent coordination](../architecture/chief-coordination.md), [approvals and native ownership](../workflows/approvals-and-native-ownership.md), [product scope](upstream-product-scope.md), and [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).
