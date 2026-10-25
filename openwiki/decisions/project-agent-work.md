---
type: Decision
title: "Project, Agent and Work contract"
description: "Accepted architecture, shared English vocabulary, minimum Work and Collaboration contracts, recovery, and global Guardian controls."
tags: ["decodex", "architecture", "agents", "contracts", "guardian"]
sources:
  - id: openwiki-source-7eedab1ccdf82ac9ccd3ab32
    resource: repo://automations/upstream/retained-evidence.json
  - id: openwiki-source-991553c8d747a54ecf5a9376
    resource: repo://crates/decodex-core/src/work.rs
  - id: openwiki-source-c75093d19a3bc72db5836102
    resource: repo://crates/decodex-runtime/src/agent_host.rs
  - id: openwiki-source-dbc533dc306a3f520c4241e0
    resource: repo://crates/decodex-runtime/src/agent.rs
  - id: openwiki-source-a4a2658e06fa2cf48ff4589e
    resource: repo://crates/decodex-runtime/src/agent/global_guardian.rs
  - id: openwiki-source-cd44bbfa50ac15a596e824a2
    resource: repo://crates/decodex-runtime/src/agent/guardian.rs
  - id: openwiki-source-2fd274a65db230908814f7da
    resource: repo://crates/decodex-runtime/src/agent/native_turns.rs
  - id: openwiki-source-d511cef2848dd5f6968572ae
    resource: repo://crates/decodex-runtime/src/agent/tool_migration.rs
  - id: openwiki-source-b1dd4c8ab85f342314daa6f1
    resource: repo://crates/decodex-runtime/src/agent/work.rs
  - id: openwiki-source-bdc10e06c0058b26a9bde83d
    resource: repo://database/migrations/0056_project_agent_work.sql
  - id: openwiki-source-3030ced79067a3721a08b107
    resource: repo://database/src/agent_native_turns.rs
  - id: openwiki-source-d1279ad243eb550afec4d0e6
    resource: repo://database/src/guardian_settings.rs
  - id: openwiki-source-28af4781ad8d9558410630da
    resource: repo://database/src/works.rs
  - id: openwiki-source-4e05748daab41d1c9c2f54dd
    resource: repo://database/src/works/collaboration.rs
  - id: openwiki-source-c0eb738b42368debe7ef7545
    resource: repo://database/src/works/commands.rs
generated: { by: "codex", at: "2026-10-25T01:55:03.097Z" }
verified:
  - by: openwiki/0.7.2
    at: 2026-10-25T01:55:03.097Z
---

# Project, Agent and Work contract

## Decision and implementation status

Status: **Accepted and implemented in the Project, Agent and Work change.** The user approved this architecture, global Guardian controls, and the English vocabulary below. This page is the canonical decision for that scope. It supersedes earlier design proposals that require fixed L0/L1/L2 management levels, mandatory routing through Main, permanent component managers, or several concurrent conversations that share one Agent identity.

This decision applies to general-purpose work, including research, planning, writing, operations, analysis and software. It does not authorize installation, release, changes to a maintenance schedule, or automatic execution of all historical optional proposals. It does not certify an installed application. Documentation alone does not establish a migration of an installed application. Review Details remains an explicit global opt-in.

The normative sections define the contract. [Current implementation and migration](#current-implementation-and-migration) describes the implementation and remaining qualification limits. Older qualification records retain their original source, artifact and behavior limits. An older record that calls Review Details unselected describes its qualification scope; the product decision on this page is now accepted.

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

Native Thread, Turn and Goal keep their distinct execution meanings and protocol names. Some legacy execution readers and viewport identifiers retain old names for actual compatibility. They do not define the business model. Project replaces Workspace in the business schema and public protocol; existing directory and membership data survives the direct migration.

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

The selected native interface must actually support each operation. A helper follow-up API must not be assumed to be a generic bus for unrelated root conversations. The [inspected native implementation](https://github.com/openai/codex/blob/970111f6cb62cc02fe36a82ea7d71b37a2889c97/codex-rs/core/src/agent/control/api.rs#L126-L147) rejects a follow-up that targets a root. The selected development artifact uses this exact source. Its affected native behavior was tested separately; source inclusion alone does not establish qualification.

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

The user selected this capability as part of the global design. Keep its opt-in in global Guardian settings, not on individual Agents. The implementation qualifies its native event subscription and configuration against the exact selected binary and helpers; future runtime changes must repeat the affected checks. Do not copy the reviewer or enforcement engine.

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
2. Each owner chooses direct execution or authorized native Subagents. The authority to delegate is explicit and does not depend on Ultra. The synthetic native matrix qualifies supported V1 and V2 paths. It does not measure live model judgment or delegation quality.
3. The user has permitted a bounded exchange of public venue and transport Materials. Sage asks Iris for those Materials for A1. The request waits while Iris is busy and becomes Accepted only when she accepts it. Her family information is not shared. This contribution is a Collaboration Request, not another permanent Agent or a fifth independent Work.
4. The user changes the market from 60 people to 80 people, fully indoors, with the same budget. Sage updates A1 and A2 and their affected instructions. B1 and B2 keep their requirements. Old venue capacity evidence cannot close A1; unchanged transport evidence can be checked for reuse.
5. Sage finds that insurance in A2 still uses 60 people and returns the same contribution for repair. Iris finds that a proposed hotel does not fit B1's accessible route and arranges a compatible revision. The original Work identities and owners remain.
6. Each owner checks current Results against the agreed criteria and closes its Works. The user can follow each Result back to requirements, Materials, Collaboration Requests and assessment. The plans do not claim that a venue or hotel has been booked.

## Current implementation and migration

Schema 56 migrates directly to Project membership, persistent Agent responsibilities and separate Work. Protocol 3.0 rejects incompatible clients explicitly. There is no dual business-name API or gradual rollout. The migration preserves genuine legacy execution, instructions, native identities, inbox receipts and historical completion limits. A migrated historical outcome is not newly qualified Result evidence. No legacy executor is relabelled as a native Subagent.

| Area | Implemented behavior |
| --- | --- |
| Agent identity | User-created independent Agent, one current native conversation. Assistant is optional; Project Lead selects an existing Agent. A legacy parent identifier does not confer business authority. |
| Work | Current immutable requirements, one owner, explicit state and holds, scoped Materials, exact Results, reuse assessment and atomic completion checks. |
| Collaboration | Stable request and instruction identities, durable busy queue, exact delivered-turn acceptance, targeted return, assessment, repair and scoped cancellation. |
| Native execution | Intent before native call, exact direct-call attribution, native messages, waits and continuation. Unbound or ambiguous evidence stays Unknown without a duplicate spawn. |
| Project | Supported native Project create/read/list and exact idempotency, with Decodex Work and responsibilities retained locally. Native Project catalog does not prove desktop saved-Project synchronization. |
| Guardian | Global Ask Me or Automatic Review, separate desired and per-source effective receipts, no editable per-Agent reviewer override. Native permissions and Full Access remain separate. |
| Review Details | Explicit global opt-in; verified private reviewer/parent identity; native assistant output and terminal metadata only. No raw reasoning frames, parent-history pollution, action replay or guessed action association. |
| Presentation | Existing desktop Work and Guardian entries, Project grouping, existing-Agent responsibility selectors, clickable Materials, Results and Collaboration evidence. |
| Compatibility | Retained real old execution, archive, forks, input, automation and evidence consumers. New default helper creation uses native tools. |

The default tool revision advertises `work_read` and `work_manage`. Managed temporary-worker creation/continuation, subordinate-manager creation and old goal/dependency tools are retired from that surface and rejected if an old schema still advertises them. Existing automation and native recovery remain; no duplicate Work controller, native mailbox or permanent review department is introduced.

An existing native tool set migrates only when its source and descendants are quiescent and inspectable. The durable journal saves replacement identity before retirement, preserves model/provider/effort/tier/permissions and history, verifies actual resumed settings and requires positive archive acknowledgment before the binding switches. Unknown creation or retirement stays parked. The old context remains read-only history; the service never resets it or blindly creates a replacement.

A Project association is business scope. A native conversation still has one actual execution directory, Project and permission boundary. Work association does not switch them. Work Pause blocks new Decodex contributions and completion; already-running native actions, Goals and children require native control. These limits are visible in the UI.

## Qualification and evidence limits

The selected development source is official Codex `970111f6cb62cc02fe36a82ea7d71b37a2889c97`. Explicit `cargo +stable make prepare-codex` verifies the actual package, CLI, Code Mode helper, search helper, provenance and generated schema. This change keeps the selected runtime baseline; it does not advance the upstream review cursor or identify the installed application. Component hashes, retained regression identities, entrypoints and reproducible commands are in [retained evidence](../../automations/upstream/retained-evidence.json), under `retained_project_agent_work`.

Executed validation for this change:

- Workspace all-target/all-feature check, strict lint for 13 packages and nightly format check passed.
- Database suite: 188 passed. The scope regression first reproduced the old failure with unrelated records ahead of the Agent's own Work and authorized Material, then passed after scope moved before the SQL limit.
- Exact-artifact native matrix: 13 passed, 985 excluded by its selector, zero unmatched selected tests. It covers native V1/V2 contributions, V2 idle wake, tool migration, Project/global reviewer/private records, child approvals and retained context/permission behavior.
- Transport and receipt suite: 28 passed, 970 excluded by selector. Focused UI/runtime checks: 6 passed. Python checks: 40 passed. CLI diagnostics: 2 passed. Local database gate passed with schema 56 and 89 tables. Counts overlap; do not add them as distinct tests.
- Original complete workspace suite: 2918 executed, 2916 passed including one leaky pass, 2 failed, 1154 skipped. Both failures timed out during native audio ICE connection. The later `native_media_followup` retains this failed result separately from fresh proof: current main passed 2918/2918, and the two media tests passed 20 repetitions on both current main and the pre-change baseline, 40 executions per source with no retries. The audio source, dependency versions and enabled WebRTC features match across those sources. Historical macOS firewall logs show queued inbound connections without a verdict during the two failures; later runs had known allow verdicts. This supports an external firewall admission cause. A locked-session trigger was not directly tested. No product source, audio assertion, deadline or runner policy changed.

The selected model catalog and configuration determine the native multi-agent backend. The synthetic matrix uses V2 with `gpt-5.6-sol` and V1 with `gpt-5.6-luna`, both at Medium. A false V2 feature flag alone does not establish V1. Qualified V2 supplies idle-owner completion wakeups; V1 requires native wait for required Results. Nested Code Mode delegation attribution remains unqualified and Unknown.

Global reviewer updates set the native thread preference for subsequent turns. Active turns, pending human approvals and already-created child captures retain native ownership. Private reviewer records lack exact action Review IDs in this runtime; the UI states the association is unavailable. It retains at most 128 records, at most 64 KiB each, previews the newest 32 and retrieves a selected exact record separately. Reconnect gaps remain explicit.

Work evidence reads apply scope before a 100-record limit per category and expose `has_more`. Selected evidence uses digest-checked 8 KiB pages up to 32 MiB. Missing, changed or oversized evidence stays unavailable. These are bounded views, not proof that the whole history was read.

Synthetic provider tests establish the measured native integration contracts. They do not establish live-account/model quality, installed-app visual or interaction acceptance, reservations, payments or other real external outcomes. Source inclusion and digest checks prove provenance, not semantic correctness. See [acceptance boundaries](../testing/upstream-acceptance-boundaries.md).
